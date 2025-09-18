use core::ptr::null_mut;

use alloc::alloc::{GlobalAlloc, Layout};
use linked_list_allocator::LockedHeap;

use x86_64::{
    structures::paging::{
        mapper::MapToError, FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB,
    },
    VirtAddr,
};

use bootloader_api::info::{MemoryRegions, MemoryRegionKind};

// Place heap in higher memory compatible with high virtual address offsets
// Use 64MB mark - safely above UEFI runtime services and low memory
// Compatible with bootloader virtual address offset (512GB range)
pub const HEAP_START: usize = 0x4000000; // 64MB mark - compatible with high virtual addresses
pub const HEAP_SIZE: usize = 1024 * 1024; // 1MB

pub struct Dummy;

unsafe impl GlobalAlloc for Dummy {
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        null_mut()
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        panic!("dealloc should be never called")
    }
}

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Comprehensive UEFI memory map validation for early boot stability
fn validate_uefi_memory_map(memory_regions: &MemoryRegions) -> Result<(), &'static str> {
    use crate::serial_println;

    let heap_start = HEAP_START as u64;
    let heap_end = heap_start + HEAP_SIZE as u64;

    serial_println!("=== UEFI Memory Map Validation ===");
    serial_println!("Heap region: 0x{:x} - 0x{:x} ({}KB)", heap_start, heap_end, HEAP_SIZE / 1024);

    // Check if heap placement is compatible with high virtual addresses
    if heap_start >= 0x4000000 {
        serial_println!("  -> Heap in high memory (64MB+) - compatible with high virtual addresses");
    } else if heap_start >= 0x1000000 {
        serial_println!("  -> Heap in medium memory (16MB+) - may have conflicts");
    } else {
        serial_println!("  -> Heap in low memory (<16MB) - likely conflicts with UEFI");
    }

    let mut usable_count = 0;
    let mut total_usable_size = 0u64;
    let mut heap_in_usable = false;

    // First pass: catalog all memory regions
    for (i, region) in memory_regions.iter().enumerate() {
        let size = region.end - region.start;
        serial_println!("Region {}: {:?} 0x{:x}-0x{:x} ({}MB)",
                       i, region.kind, region.start, region.end, size / 1024 / 1024);

        match region.kind {
            MemoryRegionKind::Usable => {
                usable_count += 1;
                total_usable_size += size;

                // Check if heap falls within this usable region
                if heap_start >= region.start && heap_end <= region.end {
                    heap_in_usable = true;
                    serial_println!("  ✓ Heap fits within this usable region");
                }
            }
            MemoryRegionKind::Bootloader => {
                serial_println!("  ⚠ Bootloader region - avoid conflicts");
            }
            MemoryRegionKind::UnknownUefi(code) => {
                serial_println!("  ⚠ Unknown UEFI region (code: {})", code);
            }
            _ => {
                serial_println!("  ℹ Other region type");
            }
        }
    }

    serial_println!("=== Memory Map Summary ===");
    serial_println!("Total usable regions: {}", usable_count);
    serial_println!("Total usable memory: {}MB", total_usable_size / 1024 / 1024);

    // Validate minimum requirements
    if usable_count == 0 {
        serial_println!("✗ FATAL: No usable memory regions found!");
        return Err("No usable memory available");
    }

    if total_usable_size < 16 * 1024 * 1024 {  // Minimum 16MB
        serial_println!("✗ FATAL: Insufficient usable memory ({}MB < 16MB required)",
                       total_usable_size / 1024 / 1024);
        return Err("Insufficient memory for kernel operation");
    }

    if !heap_in_usable {
        serial_println!("✗ FATAL: Heap region not within any usable memory!");
        return Err("Heap not in usable memory region");
    }

    // Check for dangerous overlaps
    for region in memory_regions.iter() {
        let overlaps = !(heap_end <= region.start || heap_start >= region.end);

        if overlaps {
            match region.kind {
                MemoryRegionKind::Usable => {
                    // This is expected and good
                }
                MemoryRegionKind::Bootloader => {
                    serial_println!("✗ FATAL: Heap conflicts with bootloader at 0x{:x}-0x{:x}",
                                   region.start, region.end);
                    return Err("Heap conflicts with bootloader");
                }
                MemoryRegionKind::UnknownUefi(_) |
                MemoryRegionKind::UnknownBios(_) => {
                    serial_println!("✗ FATAL: Heap conflicts with firmware region at 0x{:x}-0x{:x}",
                                   region.start, region.end);
                    return Err("Heap conflicts with firmware");
                }
                _ => {
                    serial_println!("⚠ WARNING: Heap overlaps with {:?} region 0x{:x}-0x{:x}",
                                   region.kind, region.start, region.end);
                }
            }
        }
    }

    serial_println!("✓ UEFI memory map validation completed successfully");
    Ok(())
}

pub fn init_heap(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    memory_regions: &MemoryRegions,
) -> Result<(), MapToError<Size4KiB>> {
    use crate::serial_println;

    serial_println!("Heap init: start=0x{:x}, size={} bytes", HEAP_START, HEAP_SIZE);

    // Comprehensive UEFI memory map validation
    if let Err(error) = validate_uefi_memory_map(memory_regions) {
        serial_println!("✗ FATAL: UEFI memory validation failed: {}", error);
        panic!("Memory layout incompatible with UEFI boot: {}", error);
    }

    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + HEAP_SIZE - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    serial_println!("Heap: mapping {} pages", page_range.count());

    for (i, page) in page_range.enumerate() {
        serial_println!("Mapping heap page {}: {:?}", i, page);

        serial_println!("  Allocating frame for page {}...", i);
        let frame = match frame_allocator.allocate_frame() {
            Some(frame) => {
                serial_println!("  Frame allocated: {:?}", frame);
                frame
            }
            None => {
                serial_println!("  ERROR: Frame allocation failed for page {}", i);
                return Err(MapToError::FrameAllocationFailed);
            }
        };

        serial_println!("  Mapping page {} to frame {:?}...", i, frame);
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe {
            match mapper.map_to(page, frame, flags, frame_allocator) {
                Ok(mapping) => {
                    serial_println!("  Page {} mapped successfully, flushing...", i);
                    mapping.flush();
                    serial_println!("  Page {} flush complete", i);
                }
                Err(e) => {
                    serial_println!("  ERROR: Failed to map page {}: {:?}", i, e);
                    return Err(e);
                }
            }
        };

        serial_println!("  Page {} mapping complete", i);
    }

    serial_println!("Heap pages mapped successfully, initializing allocator...");

    unsafe {
        ALLOCATOR.lock().init(HEAP_START, HEAP_SIZE);
    }

    serial_println!("Heap allocator initialized successfully");
    Ok(())
}