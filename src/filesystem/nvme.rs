use core::fmt::Write;

use x86_64::structures::paging::{FrameAllocator, OffsetPageTable, Size4KiB};
use x86_64::{VirtAddr};

use crate::vga_buffer::Writer;
use crate::{log, serial_println};
use crate::memory::map_nvme_base;
use crate::hardware::PCI::read_pci_bar;

static mut NVME_VIRT_ADDR: Option<VirtAddr> = None;

pub fn init_controller(mapper: &mut OffsetPageTable, frame_allocator: &mut impl FrameAllocator<Size4KiB>) {
    let nvme_base_addr = find_first_nvme();
    let nvme_virt_addr = VirtAddr::new(0xffff_8000_0000_0000 + nvme_base_addr);

    map_nvme_base(nvme_base_addr, nvme_virt_addr, mapper, frame_allocator);

    reset_nvme(nvme_virt_addr.as_u64());

    unsafe {
        NVME_VIRT_ADDR = Some(nvme_virt_addr);
    }
}

fn reset_nvme(addr: u64) {
    let cap = nvme_read_reg64(0x00, addr);
    let timeout = (cap >> 24) & 0xF;

    nvme_write_reg(addr, 0x14, 0);
    serial_println!("Succesfully Reset");

}

pub fn find_first_nvme() -> u64 {
    for bus in 0..=255 {
        for device in 0..31 {
            for function in 0..7 {
                if let Some(pci_device) = crate::hardware::PCI::get_pci_device(bus, device, function) {
                    if pci_device.class_code == 0x01 && pci_device.subclass_code == 0x08 {
                        let base_adr = get_nvme_base_addr(bus, device, function) as u64;

                        return base_adr;
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
            nvme_write_reg(nvme_virt_addr.as_u64(), 0x14, 0x1);

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
    let bar1 = read_pci_bar(bus, device, function, 1); //

    // Combineer BAR0 en BAR1 tot een 64-bit MMIO-adres
    let nvme_base_addr = ((bar1 as u64) << 32) | (bar0 as u64 & 0xFFFFFFF0);
    nvme_base_addr
}

// Read & Write NVME reg
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

fn nvme_write_reg(nvme_virt_addr: u64, offset: u32, value: u32) {
    unsafe {
        let reg_addr = (nvme_virt_addr + offset as u64) as *mut u32;
        core::ptr::write_volatile(reg_addr, value);
        serial_println!("Value {} has been written to {:?}", value, reg_addr);
    }
}