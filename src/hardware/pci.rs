use alloc::vec::Vec;
use x86_64::instructions::port::Port;
use crate::{log, serial_println, vga_buffer};
use vga_buffer::Writer;

#[derive(Debug, Clone, Copy)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass_code: u8,
    pub prog_if: u8,
    pub revision_id: u8,
}

fn pci_config_address(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let bus = bus as u32;
    let device = device as u32;
    let function = function as u32;
    let offset = offset as u32;

    0x8000_0000 | (bus << 16) | (device << 11) | (function << 8) | (offset & 0xFC)
}

fn read_pci_config_word(address: u32) -> u16 {
    let mut address_port = Port::<u32>::new(0xCF8);
    let mut data_port = Port::<u32>::new(0xCFC);

    unsafe {
        address_port.write(address);
        (data_port.read() & 0xFFFF) as u16
    }
}

fn read_pci_config_byte(address: u32) -> u8 {
    let mut address_port = Port::<u32>::new(0xCF8);
    let mut data_port = Port::<u32>::new(0xCFC);

    unsafe {
        address_port.write(address & 0xFFFFFFFC); // Align address to 32-bit boundary
        let data = data_port.read();
        let shift = ((address & 0x03) * 8) as u32;
        ((data >> shift) & 0xFF) as u8
    }
}

fn get_pci_device(bus: u8, device: u8, function: u8) -> Option<PciDevice> {
    let address = pci_config_address(bus, device, function, 0x00);
    let vendor_id = read_pci_config_word(address);

    if vendor_id == 0xFFFF {
        // No device present
        return None;
    }

    let device_id = read_pci_config_word(address + 2);
    let class_code = read_pci_config_byte(address + 0x0B);
    let subclass_code = read_pci_config_byte(address + 0x0A);
    let prog_if = read_pci_config_byte(address + 0x09);
    let revision_id = read_pci_config_byte(address + 0x08);

    Some(PciDevice {
        bus,
        device,
        function,
        vendor_id,
        device_id,
        class_code,
        subclass_code,
        prog_if,
        revision_id,
    })
}

// Updated function to get more detailed PCI information and print it to the console (VGA buffer)
pub fn debug_pci_scan(writer: &mut Writer) {
    for bus in 0..=1 {  // Start with scanning only the first two buses for now
        for device in 0..2 {  // Scan the first five devices
            for function in 0..2 {  // Scan the first two functions for more comprehensive coverage
                log!(writer, "Scanning: Bus {}, Device {}, Function {}", bus, device, function);
                if let Some(pci_device) = get_pci_device(bus, device, function) {
                    log!(
                        writer,
                        "Found PCI Device: Bus {}, Device {}, Function {}, Vendor ID: {:04x}, Device ID: {:04x}, Class Code: {:02x}, Subclass Code: {:02x}, Prog IF: {:02x}, Revision ID: {:02x}",
                        pci_device.bus,
                        pci_device.device,
                        pci_device.function,
                        pci_device.vendor_id,
                        pci_device.device_id,
                        pci_device.class_code,
                        pci_device.subclass_code,
                        pci_device.prog_if,
                        pci_device.revision_id
                    );
                } else {
                    log!(writer, "No device found at Bus {}, Device {}, Function {}", bus, device, function);
                }
            }
        }
    }
}

// Functie om de PCI-bus te scannen
pub fn display_disks(writer: &mut Writer) {

    log!(writer, "Start scanning PCI Bus for Mass Storage Controllers...");

    debug_pci_scan(writer);

    serial_println!("PCI scan completed, storage devices displayed.");
}