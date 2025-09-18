use uart_16550::SerialPort;
use spin::Mutex;
use lazy_static::lazy_static;
use crate::serial_print;

lazy_static! {
    pub static ref SERIAL1: Mutex<SerialPort> = {
        let mut serial_port = unsafe { SerialPort::new(0x3F8) };
        serial_port.init();
        Mutex::new(serial_port)
    };
}

/// Early boot-safe serial initialization check
pub fn init_serial() -> Result<(), &'static str> {
    // Try to initialize serial port early for debugging
    // This is safe to call multiple times
    lazy_static::initialize(&SERIAL1);

    // Send a test message to verify serial is working
    serial_print!("Serial port initialized for early boot debugging");
    Ok(())
}

#[doc(hidden)]
pub fn _print(args: ::core::fmt::Arguments) {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;

    // Use direct Write trait to avoid heap allocations during early boot
    interrupts::without_interrupts(|| {
        SERIAL1.lock().write_fmt(args).expect("Serial write failed");
    });
}

/// Prints to the host through the serial interface.
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::serial::_print(format_args!($($arg)*));
    };
}

/// Prints to the host through the serial interface, appending a newline.
#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($fmt:expr) => ($crate::serial_print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial_print!(
        concat!($fmt, "\n"), $($arg)*));
}