use x86_64::{
    structures::paging::{FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags as Flags, PhysFrame, Size4KiB},
    PhysAddr,
    VirtAddr,
};

use bootloader_api::info::{MemoryRegions, MemoryRegionKind};
use crate::hardware::pit::{pit_init};
use crate::hardware::rdsp::find_rsdp;
use crate::serial_println;

pub struct BootInfoFrameAllocator {
    memory_regions: &'static MemoryRegions,
    next: usize,
}

impl BootInfoFrameAllocator {
    pub unsafe fn init(memory_regions: &'static MemoryRegions) -> Self {
        BootInfoFrameAllocator {
            memory_regions,
            next: 0,
        }
    }
}

impl BootInfoFrameAllocator {
    fn usable_frames(&self) -> impl Iterator<Item = PhysFrame> {
        use crate::serial_println;

        // get usable regions from memory regions
        let regions = self.memory_regions.iter();
        let usable_regions = regions
            .filter(|r| {
                match r.kind {
                    MemoryRegionKind::Usable => {
                        // Additional validation for usable regions
                        let size = r.end - r.start;
                        if size < 4096 {
                            serial_println!("⚠ Skipping tiny usable region: 0x{:x}-0x{:x} ({}B)",
                                          r.start, r.end, size);
                            false
                        } else {
                            serial_println!("✓ Using memory region: 0x{:x}-0x{:x} ({}MB)",
                                          r.start, r.end, size / 1024 / 1024);
                            true
                        }
                    }
                    _ => {
                        serial_println!("✗ Skipping non-usable region: {:?} 0x{:x}-0x{:x}",
                                      r.kind, r.start, r.end);
                        false
                    }
                }
            });

        // map each region to its address range
        let addr_ranges = usable_regions
            .map(|r| r.start..r.end);

        // transform to an iterator of frame start addresses
        let frame_addresses = addr_ranges.flat_map(|r| r.step_by(4096));

        // create `PhysFrame` types from the start addresses
        frame_addresses
            .map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
            .filter(|frame| {
                let addr = frame.start_address().as_u64();

                // CRITICAL: Skip frame 0 - it's reserved and causes hangs
                if addr == 0 {
                    return false;
                }

                // Additional safety checks for UEFI systems
                // Avoid the first 1MB (BIOS/UEFI reserved areas)
                if addr < 0x100000 {
                    return false;
                }

                // Avoid potential video memory areas (0xA0000-0xFFFFF)
                if addr >= 0xA0000 && addr < 0x100000 {
                    return false;
                }

                true
            })
    }
}

pub struct EmptyFrameAllocator;

unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        use crate::serial_println;

        let frame = self.usable_frames().nth(self.next);
        match &frame {
            Some(f) => {
                serial_println!("📦 Allocated frame {} at 0x{:x}",
                              self.next, f.start_address().as_u64());
            }
            None => {
                serial_println!("⚠ Frame allocation failed at index {}", self.next);
                serial_println!("   Available frames exhausted or no usable memory found");
            }
        }
        self.next += 1;
        frame
    }
}

pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    use crate::serial_println;

    serial_println!("🗺️  Initializing memory mapper with offset: {:?}", physical_memory_offset);
    serial_println!("   Accessing level 4 page table...");

    let level_4_table = active_level_4_table(physical_memory_offset);
    serial_println!("   ✓ Level 4 page table accessed successfully");

    serial_println!("   Creating OffsetPageTable...");
    let mapper = OffsetPageTable::new(level_4_table, physical_memory_offset);
    serial_println!("   ✓ OffsetPageTable created successfully");

    mapper
}

unsafe fn active_level_4_table(physical_memory_offset: VirtAddr)
    -> &'static mut PageTable
{
    use x86_64::registers::control::Cr3;
    use crate::serial_println;

    serial_println!("     Reading CR3 register...");
    let (level_4_table_frame, _) = Cr3::read();
    let phys = level_4_table_frame.start_address();
    serial_println!("     CR3 points to physical frame: 0x{:x}", phys.as_u64());

    serial_println!("     Converting to virtual address...");
    let virt = physical_memory_offset + phys.as_u64();
    serial_println!("     Virtual address: 0x{:x} (phys + offset)", virt.as_u64());

    // Validate the virtual address before using it
    serial_println!("     Validating virtual address range...");
    if virt.as_u64() > 0xFFFFFFFFFFFFFFFF {
        panic!("FATAL: Virtual address overflow");
    }

    // Check if address is in canonical range
    let addr = virt.as_u64();
    if addr > 0x0000_7FFF_FFFF_FFFF && addr < 0xFFFF_8000_0000_0000 {
        serial_println!("     WARNING: Address in non-canonical range!");
        panic!("FATAL: Non-canonical virtual address for page table");
    }

    serial_println!("     Creating page table pointer...");
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();
    serial_println!("     ✓ Page table pointer created: {:p}", page_table_ptr);

    serial_println!("     Attempting to dereference page table pointer...");
    // This is the critical line that might cause the hang
    let page_table = &mut *page_table_ptr;
    serial_println!("     ✓ Page table dereferenced successfully");

    page_table
}

pub unsafe fn translate_addr(addr: VirtAddr, physical_memory_offset: VirtAddr)
    -> Option<PhysAddr>
{
    translate_addr_inner(addr, physical_memory_offset)
}

fn translate_addr_inner(addr: VirtAddr, physical_memory_offset: VirtAddr)
                        -> Option<PhysAddr>
{
    use x86_64::structures::paging::page_table::FrameError;
    use x86_64::registers::control::Cr3;

    // read the active level 4 frame from the CR3 register
    let (level_4_table_frame, _) = Cr3::read();

    let table_indexes = [
        addr.p4_index(), addr.p3_index(), addr.p2_index(), addr.p1_index()
    ];
    let mut frame = level_4_table_frame;

    // traverse the multi-level page table
    for &index in &table_indexes {
        // convert the frame into a page table reference
        let virt = physical_memory_offset + frame.start_address().as_u64();
        let table_ptr: *const PageTable = virt.as_ptr();
        let table = unsafe {&*table_ptr};

        // read the page table entry and update `frame`
        let entry = &table[index];
        frame = match entry.frame() {
            Ok(frame) => frame,
            Err(FrameError::FrameNotPresent) => return None,
            Err(FrameError::HugeFrame) => panic!("huge pages not supported"),
        };
    }

    // calculate the physical address by adding the page offset
    Some(frame.start_address() + u64::from(addr.page_offset()))
}

pub fn map_nvme_base(
    nvme_base_addr: u64,
    virt_addr: VirtAddr,
    mapper: &mut OffsetPageTable,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) {
    let frame: PhysFrame<Size4KiB> = PhysFrame::containing_address(PhysAddr::new(nvme_base_addr));
    let page: Page<Size4KiB> = Page::containing_address(virt_addr);
    let flags = Flags::PRESENT | Flags::WRITABLE;

    let map_to_result = unsafe {
        mapper.map_to(page, frame, flags, frame_allocator)
    };

    serial_println!("{:?}", map_to_result);

    map_to_result.expect("map_to failed").flush();
}

pub fn map_bios_area(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>
) {
    let bios_start = PhysAddr::new(0xE0000); // Start of BIOS memory
    let bios_end = PhysAddr::new(0xFFFFF);   // End of BIOS memory
    let bios_size = bios_end.as_u64() - bios_start.as_u64() + 1;

    let num_pages = (bios_size / 4096) as usize; // Number of pages to map

    // INIT ACPI
    for i in 0..num_pages {
        let frame = PhysFrame::containing_address(bios_start + i as u64 * 4096);
        let page = Page::containing_address(VirtAddr::new(bios_start.as_u64() + i as u64 * 4096));

        // Map each page
        unsafe {
            mapper.map_to(page, frame, Flags::PRESENT | Flags::WRITABLE, frame_allocator)
                .expect("Mapping failed")
                .flush();
        }
    }

    //INIT RSDT
    if let Some(rsdp) = find_rsdp() {
        let rsdt_address = rsdp.rsdt_address as u64;
        map_rsdt_area(rsdt_address, mapper, frame_allocator);
    }

    //INIT PIT
    pit_init();
}

pub fn map_rsdt_area(
    rsdt_address: u64,
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>
) {
    let frame = PhysFrame::containing_address(PhysAddr::new(rsdt_address));
    let page = Page::containing_address(VirtAddr::new(rsdt_address));

    unsafe {
        // Map the page containing the RSDT into virtual memory
        mapper.map_to(page, frame, Flags::PRESENT | Flags::WRITABLE, frame_allocator)
            .expect("Failed to map RSDT")
            .flush();
    }
}
