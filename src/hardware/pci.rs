use crate::{log, serial_println, vga_buffer};
use vga_buffer::Writer;

// Functie om de PCI-bus te scannen
pub fn scan_pci_bus(writer: &mut Writer) {
    serial_println!("Trying");

    log!(writer, "maybe this works");

    serial_println!("No deadlock. Do you see a message? If not, something else is wrong.");
}