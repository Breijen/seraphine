use x86_64::instructions::port::Port;

pub fn disable_hardware_cursor() {
    unsafe {
        let mut port = Port::new(0x3D4);
        port.write(0x0A_u8); // Cursor Start Register
        let mut data_port = Port::new(0x3D5);
        data_port.write(0x20_u8); // Zet de hoogste bit om de cursor te verbergen
    }
}