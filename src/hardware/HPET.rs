use x86_64::VirtAddr;
use crate::serial_println;

#[repr(C, packed)]
pub struct ACPISDTHeader {
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
pub struct HpetTable {
    header: ACPISDTHeader,
    event_timer_block_id: u32,
    base_address: u64,       // Base Address van de HPET-registers
    hpet_number: u8,
    minimum_tick: u16,
    page_protection: u8,
}

static mut HPET_VIRT_ADDR: Option<VirtAddr> = None;

/// Zoek naar de HPET-tabel in de RSDT/XSDT
pub fn find_hpet_in_rsdt(rsdt_address: u32) -> Option<u64> {
    let rsdt_header = unsafe { &*(rsdt_address as *const ACPISDTHeader) };

    let num_entries = (rsdt_header.length as usize - core::mem::size_of::<ACPISDTHeader>()) / 4;

    for i in 0..num_entries {
        let entry_ptr = unsafe { (rsdt_address as *const u32).add(core::mem::size_of::<ACPISDTHeader>() / 4 + i) };
        let entry = unsafe { *entry_ptr };
        let sdt_header = unsafe { &*(entry as *const ACPISDTHeader) };

        // Check for HPET signature
        if &sdt_header.signature == b"HPET" {
            return Some(entry as u64); // Return the physical address of the HPET table
        }
    }

    None
}

/// Lees de HPET-tabel
pub fn read_hpet_table(hpet_address: u64) -> Option<&'static HpetTable> {
    let hpet_table = unsafe { &*(hpet_address as *const HpetTable) };

    if &hpet_table.header.signature == b"HPET" {
        Some(hpet_table)
    } else {
        None
    }
}

/// Initialiseer de HPET met behulp van het basisadres
pub fn init_hpet(hpet_addr: &VirtAddr) {
    let addr = (hpet_addr).as_u64() as *mut u64;

    serial_println!("{:?}", addr);

    // Schrijf naar het General Configuration Register om HPET in te schakelen
    unsafe {
        hpet_write_reg(addr as u64, 0x10, 1);

        serial_println!("HPET enabled with configuration: 1");
    }
}

/// Vind en initialiseer de HPET
pub fn init_hpet_addr(virt_addr: VirtAddr) {
    unsafe {
        HPET_VIRT_ADDR = Some(virt_addr);

        if let Some(ref virt_addr) = HPET_VIRT_ADDR {
            init_hpet(virt_addr); // Pass the reference to the actual VirtAddr
        }
    };
}

fn hpet_write_reg(nvme_virt_addr: u64, offset: u32, value: u64) {
    unsafe {
        let reg_addr = (nvme_virt_addr + offset as u64) as *mut u64;
        serial_println!("Writing value {:#x} to HPET 64-bit register at address {:#x}", value, reg_addr as u64);
        core::ptr::write_volatile(reg_addr, value);
    }
}