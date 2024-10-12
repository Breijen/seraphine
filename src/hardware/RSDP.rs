use crate::serial_println;

#[repr(C, packed)]
struct ACPISDTHeader {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: u32,
    creator_revision: u32,
}

#[repr(C, packed)]
pub struct Rsdp {
    signature: [u8; 8],   // "RSD PTR "
    checksum: u8,
    oem_id: [u8; 6],
    revision: u8,         // ACPI version
    pub rsdt_address: u32,    // RSDT pointer (32-bit)
    // If revision >= 2, these fields are available:
    length: u32,          // Length of the entire RSDP (ACPI 2.0+)
    xsdt_address: u64,    // XSDT pointer (64-bit)
    extended_checksum: u8,
    reserved: [u8; 3],
}

/// Zoek naar de RSDP in het geheugenbereik 0xE0000 - 0xFFFFF (BIOS RAM)
pub fn find_rsdp() -> Option<&'static Rsdp> {
    let start_address: u64 = 0xE0000;
    let end_address: u64 = 0xFFFFF;

    for address in (start_address..end_address).step_by(16) {
        let rsdp = unsafe { &*(address as *const Rsdp) };
        if &rsdp.signature == b"RSD PTR " {
            return Some(rsdp);
        }
    }

    None
}

fn print_rsdp(rsdp: &Rsdp) {
    // Convert signature and oem_id arrays to strings for printing
    let signature_str = core::str::from_utf8(&rsdp.signature).unwrap_or("Invalid signature");
    let oem_id_str = core::str::from_utf8(&rsdp.oem_id).unwrap_or("Invalid OEM ID");

    // Copy packed fields to properly aligned local variables
    let rsdt_address = rsdp.rsdt_address;
    let length = rsdp.length;
    let xsdt_address = rsdp.xsdt_address;

    // Print the basic fields of the RSDP
    serial_println!("RSDP Found:");
    serial_println!("  Signature: {}", signature_str);
    serial_println!("  Checksum: {:#x}", rsdp.checksum);
    serial_println!("  OEM ID: {}", oem_id_str);
    serial_println!("  Revision: {}", rsdp.revision);
    serial_println!("  RSDT Address: {:#x}", rsdt_address);

    // If ACPI revision >= 2.0, print additional fields
    if rsdp.revision >= 2 {
        serial_println!("  Length: {}", length);
        serial_println!("  XSDT Address: {:#x}", xsdt_address);
        serial_println!("  Extended Checksum: {:#x}", rsdp.extended_checksum);
    }
}

fn print_rsdt(rsdt_address: u32) {
    let rsdt_header = unsafe { &*(rsdt_address as *const ACPISDTHeader) };

    // Copy the fields to local variables before printing
    let length = rsdt_header.length;
    let oem_revision = rsdt_header.oem_revision;
    let creator_id = rsdt_header.creator_id;
    let creator_revision = rsdt_header.creator_revision;

    // Print RSDT header details
    let signature_str = core::str::from_utf8(&rsdt_header.signature).unwrap_or("Invalid Signature");
    serial_println!("RSDT Signature: {}", signature_str);
    serial_println!("RSDT Length: {}", length);
    serial_println!("RSDT OEM ID: {}", core::str::from_utf8(&rsdt_header.oem_id).unwrap_or("Invalid OEM ID"));
    serial_println!("RSDT OEM Table ID: {}", core::str::from_utf8(&rsdt_header.oem_table_id).unwrap_or("Invalid Table ID"));
    serial_println!("RSDT OEM Revision: {:#x}", oem_revision);
    serial_println!("RSDT Creator ID: {:#x}", creator_id);
    serial_println!("RSDT Creator Revision: {:#x}", creator_revision);
}

pub fn find_and_print_rsdp() {
    if let Some(rsdp) = find_rsdp() {
        print_rsdp(rsdp);
    } else {
        serial_println!("RSDP not found.");
    }
}

pub fn find_and_print_rsdt() {
    if let Some(rsdp) = find_rsdp() {
        let rsdt_address = rsdp.rsdt_address;
        serial_println!("RSDT Address from RSDP: {:#x}", rsdt_address);

        print_rsdt(rsdt_address);

    } else {
        serial_println!("RSDP not found.");
    }
}