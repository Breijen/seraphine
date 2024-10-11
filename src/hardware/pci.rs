use core::fmt::Write;
use vga_buffer::Writer;
use crate::{log, serial_println, vga_buffer};

use x86_64::instructions::port::Port;

use crate::filesystem::nvme::read_nvme;


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

enum StorageCodes {
    IDE,
    SATA,
    NVMe,
    Unknown,
}

impl StorageCodes {
    fn from_subclass(subclass: u8) -> Self {
        match subclass {
            0x01 => StorageCodes::IDE,
            0x06 => StorageCodes::SATA,
            0x08 => StorageCodes::NVMe,
            _ => StorageCodes::Unknown,
        }
    }

    fn to_string(&self) -> &'static str {
        match self {
            StorageCodes::IDE => "IDE",
            StorageCodes::SATA => "SATA",
            StorageCodes::NVMe => "NVMe",
            StorageCodes::Unknown => "Unknown",
        }
    }
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

fn read_pci_config_dword(address: u32) -> u32 {
    let mut address_port = Port::<u32>::new(0xCF8);
    let mut data_port = Port::<u32>::new(0xCFC);

    unsafe {
        address_port.write(address);
        data_port.read()
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

pub fn read_pci_bar(bus: u8, device: u8, function: u8, bar_num: u8) -> u32 {
    let bar_offset = 0x10 + (bar_num * 4);
    let address = pci_config_address(bus, device, function, bar_offset);
    let bar_value = read_pci_config_dword(address) as u32;  // Lees de BAR-waarde
    bar_value
}

pub(crate) fn get_pci_device(bus: u8, device: u8, function: u8) -> Option<PciDevice> {
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

pub fn debug_storage_scan(writer: &mut Writer) {
    for bus in 0..=255 {
        for device in 0..31 {
            for function in 0..7 {
                if let Some(pci_device) = get_pci_device(bus, device, function) {
                    if pci_device.class_code == 0x01 {
                        let storage_type = StorageCodes::from_subclass(pci_device.subclass_code);
                        log!(
                            writer,
                            "Found PCI Storage Device: Bus {}, Device {}, Function {}, Vendor ID: {:04x}, Device ID: {:04x}, Class Code: {:02x}, Subclass Code: {:02x}, Prog IF: {:02x}, Revision ID: {:02x}, Type: {}",
                            pci_device.bus,
                            pci_device.device,
                            pci_device.function,
                            pci_device.vendor_id,
                            pci_device.device_id,
                            pci_device.class_code,
                            pci_device.subclass_code,
                            pci_device.prog_if,
                            pci_device.revision_id,
                            storage_type.to_string()
                        );
                    }
                }
            }
        }
    }
}

// Functie om de PCI-bus te scannen
pub fn display_disks(writer: &mut Writer) {

    log!(writer, "Start scanning PCI Bus for Mass Storage Controllers...");

    read_nvme(writer);

    serial_println!("PCI scan completed, storage devices displayed.");
}