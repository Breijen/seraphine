use core::fmt::Write;
use x86_64::structures::paging::{FrameAllocator, Mapper, OffsetPageTable, Page, PageTableFlags, PhysFrame, Size4KiB};
use x86_64::{PhysAddr, VirtAddr};

use crate::vga_buffer::Writer;
use crate::{log, serial_println};
use crate::hardware::pci::read_pci_bar;
use crate::hardware::pit::timer_wait_ms;
use crate::mem::memory::map_nvme_base;

static mut NVME_VIRT_ADDR: Option<VirtAddr> = None;

const NVME_RESET_TIMEOUT: u8 = 100;
const NVME_IDENTIFY_CNS: u32 = 1;
const QUEUE_SIZE: u32 = 256; // Maximale queue grootte
const ASQ_SIZE: usize = 64 * 64; // Admin Submission Queue grootte
const ACQ_SIZE: usize = 64 * 16;  // Admin Completion Queue grootte

pub fn init_controller(mapper: &mut OffsetPageTable, frame_allocator: &mut impl FrameAllocator<Size4KiB>) {
    let nvme_base_addr = find_first_nvme();
    let nvme_virt_addr = VirtAddr::new(0xffff_8000_0000_0000 + nvme_base_addr);

    // Map NVMe base address to virtual address space
    map_nvme_base(nvme_base_addr, nvme_virt_addr, mapper, frame_allocator);

    // Initialize NVMe controller
    reset_nvme(nvme_virt_addr.as_u64());
    init_nvme_admin_queues(nvme_virt_addr.as_u64(), mapper, frame_allocator);
    enable_nvme(nvme_virt_addr.as_u64());

    // Identify the controller and record maximum transfer size
    // let (is_io_controller, max_transfer_size) = identify_controller(nvme_virt_addr.as_u64(), NVME_IDENTIFY_CNS);


    // Identify namespaces
    // identify_namespaces(nvme_virt_addr.as_u64());

    unsafe {
        NVME_VIRT_ADDR = Some(nvme_virt_addr);
    }
}

fn init_nvme_admin_queues(virt_addr: u64, mapper: &mut OffsetPageTable, frame_allocator: &mut impl FrameAllocator<Size4KiB>) {
    let asq_frame = allocate_frame(frame_allocator, "ASQ").expect("Failed to allocate ASQ frame");
    let acq_frame = allocate_frame(frame_allocator, "ACQ").expect("Failed to allocate ACQ frame");

    // Configure Admin Submission Queue and Admin Completion Queue
    configure_nvme_queues(virt_addr, asq_frame, acq_frame, mapper, frame_allocator);

    // Set queue sizes in the AQA register
    let queue_size = (QUEUE_SIZE - 1) | ((QUEUE_SIZE - 1) << 16);
    nvme_write_reg32(virt_addr, 0x24, queue_size);  // AQA register

    serial_println!("NVMe Admin Queue initialized");
}

fn identify_controller(addr: u64, cns: u32) -> (bool, u32) {
    nvme_write_reg32(addr, 0x0C, cns); // Send identify command

    let status = nvme_read_reg32(0x1C, addr);
    serial_println!("Identify status: {:?}", status);

    // Assuming that the controller is an I/O controller if the status is 0
    let is_io_controller = (status & 0x01) != 0; // Example check for I/O controller

    // Read the maximum transfer size from the CAP register
    let cap = nvme_read_reg64(0x00, addr);
    let max_transfer_size = (cap >> 32) as u32; // Maximum transfer size is usually in bits 32:63 of CAP

    (is_io_controller, max_transfer_size)
}

fn identify_namespaces(addr: u64) {
    // Read the number of namespaces
    let nsid_count = nvme_read_reg32(0x0C, addr); // Example register for namespace count
    serial_println!("Number of namespaces: {}", nsid_count);

    for nsid in 1..=nsid_count {
        nvme_identify_namespace(addr, nsid);
    }
}

fn nvme_identify_namespace(addr: u64, nsid: u32) {
    // Send Identify Namespace command
    nvme_write_reg32(addr, 0x0C, nsid); // Assume this is the command register for Identify Namespace
    let status = nvme_read_reg32(0x1C, addr);
    serial_println!("Identify Namespace {} status: {:?}", nsid, status);

/*    // Read the namespace information (assume we know the structure)
    let block_size = nvme_read_reg32(addr as u32, 0x100); // Example offset for block size
    let capacity = nvme_read_reg64(addr as u32, 0x108); // Example offset for capacity
    let read_only = (nvme_read_reg32(addr as u32, 0x110) & 0x1) != 0; // Example check for read-only status

    serial_println!("Block Size: {}", block_size);
    serial_println!("Capacity: {}", capacity);
    serial_println!("Read-Only: {}", read_only);*/
}

fn allocate_frame(frame_allocator: &mut impl FrameAllocator<Size4KiB>, name: &str) -> Result<PhysFrame<Size4KiB>, &'static str> {
    frame_allocator.allocate_frame().ok_or_else(|| {
        serial_println!("Failed to allocate frame for {}", name);
        "Allocation Error"
    })
}

fn configure_nvme_queues(virt_addr: u64, asq_frame: PhysFrame<Size4KiB>, acq_frame: PhysFrame<Size4KiB>, mapper: &mut OffsetPageTable, frame_allocator: &mut impl FrameAllocator<Size4KiB>) {
    let asq_addr = nvme_read_reg32(0x28, virt_addr);
    let acq_addr = nvme_read_reg32(0x30, virt_addr);

    map_nvme_queue(mapper, asq_frame, asq_addr, "ASQ", frame_allocator);
    map_nvme_queue(mapper, acq_frame, acq_addr, "ACQ", frame_allocator);

    // Set the physical base addresses in the NVMe controller
    nvme_write_reg64(virt_addr, 0x28, asq_frame.start_address().as_u64());  // ASQ register
    nvme_write_reg64(virt_addr, 0x30, acq_frame.start_address().as_u64());  // ACQ register
}

fn map_nvme_queue(mapper: &mut OffsetPageTable, frame: PhysFrame<Size4KiB>, addr: u32, name: &str, frame_allocator: &mut impl FrameAllocator<Size4KiB>) {
    let page: Page<Size4KiB> = Page::containing_address(VirtAddr::new(addr as u64));
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    unsafe {
        mapper.map_to(page, frame, flags, frame_allocator)
            .expect("Failed to map")
            .flush();
    }
}

fn reset_nvme(addr: u64) {
    // Read the CAP register
    let cap = nvme_read_reg64(0x00, addr);

    // Reset the NVMe controller
    nvme_write_reg32(addr, 0x14, 0); // Reset command
    serial_println!("Sent reset command to NVMe");

    // Wait until the reset is complete
    wait_nvme_reset(addr);
}

fn wait_nvme_reset(addr: u64) {
    let mut timeout = NVME_RESET_TIMEOUT;

    while timeout > 0 {
        timer_wait_ms(1);  // Wait 1 ms
        let status = nvme_read_reg32(0x1C, addr);

        if status == 0 {
            nvme_write_reg32(addr, 0x14, 1);  // Power on the controller
            return;
        }

        timeout -= 1;
    }

    serial_println!("NVMe reset timed out");
}

fn enable_nvme(addr: u64) {
    timer_wait_ms(1);  // Wait for a moment

    let status = nvme_read_reg32(0x1C, addr);
    serial_println!("STATUS: {:?}", status);

    if status == 1 {
        serial_println!("Successfully Reset NVMe");
    } else {
        serial_println!("NVMe enable failed with status: {:?}", status);
    }
}

pub fn find_first_nvme() -> u64 {
    for bus in 0..=255 {
        for device in 0..31 {
            for function in 0..7 {
                if let Some(pci_device) = crate::hardware::pci::get_pci_device(bus, device, function) {
                    if pci_device.class_code == 0x01 && pci_device.subclass_code == 0x08 {
                        return get_nvme_base_addr(bus, device, function) as u64;
                    }
                }
            }
        }
    }

    0
}

pub fn read_nvme(writer: &mut Writer) {
    unsafe {
        if let Some(nvme_virt_addr) = NVME_VIRT_ADDR {
            nvme_write_reg64(nvme_virt_addr.as_u64(), 0x14, 0x1); // Start the NVMe

            let status = nvme_read_reg32(0x1C, nvme_virt_addr.as_u64());
            log!(writer, "STATUS: {}", status);

            // Read the full 64-bit CAP register
            let cap = nvme_read_reg64(0x00, nvme_virt_addr.as_u64());
            log!(writer, "CAP: {:x}", cap);
            log!(writer, "VS: {:x}", nvme_read_reg32(0x08, nvme_virt_addr.as_u64()));

            if (status & 0x2) != 0 {
                log!(writer, "Controller in fatal status!");
            }
        } else {
            log!(writer, "NVMe controller is not initialized or mapped.");
        }
    }
}

fn get_nvme_base_addr(bus: u8, device: u8, function: u8) -> u64 {
    let bar0 = read_pci_bar(bus, device, function, 0); // BAR0
    let bar1 = read_pci_bar(bus, device, function, 1); // BAR1

    // Combine BAR0 and BAR1 into a 64-bit MMIO address
    ((bar1 as u64) << 32) | (bar0 as u64 & 0xFFFFFFF0)
}

// Read & Write NVMe registers
fn nvme_read_reg32(offset: u32, nvme_virt_addr: u64) -> u32 {
    unsafe {
        let nvme_reg = (nvme_virt_addr + offset as u64) as *const u32;
        core::ptr::read_volatile(nvme_reg)
    }
}

fn nvme_read_reg64(offset: u32, nvme_virt_addr: u64) -> u64 {
    unsafe {
        let nvme_reg = (nvme_virt_addr + offset as u64) as *const u64;
        core::ptr::read_volatile(nvme_reg)
    }
}

fn nvme_write_reg32(nvme_virt_addr: u64, offset: u32, value: u32) {
    unsafe {
        let reg_addr = (nvme_virt_addr + offset as u64) as *mut u32;
        core::ptr::write_volatile(reg_addr, value);
    }
}

fn nvme_write_reg64(nvme_virt_addr: u64, offset: u32, value: u64) {
    unsafe {
        let reg_addr = (nvme_virt_addr + offset as u64) as *mut u64;
        core::ptr::write_volatile(reg_addr, value);
    }
}
