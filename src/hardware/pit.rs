use x86_64::instructions::port::Port;
use crate::serial_println;

const PIT_FREQUENCY: u64 = 1_193_182;
const PIT_HZ: u64 = 100;
const PIT_COMMAND_PORT: u16 = 0x43;
const PIT_CHANNEL_0_PORT: u16 = 0x40;
const PIT_MODE_2: u8 = 0b00110100;

static mut TIMER_TICKS: u64 = 0;

pub fn pit_init() {
    let divisor = (PIT_FREQUENCY / PIT_HZ) as u16;

    let mut command_port = Port::<u8>::new(PIT_COMMAND_PORT);
    let mut channel_port = Port::<u8>::new(PIT_CHANNEL_0_PORT);

    unsafe {
        command_port.write(0x36);
        channel_port.write((divisor & 0xFF) as u8);
        channel_port.write((divisor >> 8) as u8);
    }
}

pub fn timer_handler() {
    unsafe {
        TIMER_TICKS += 1;

        if TIMER_TICKS % PIT_HZ == 0
        {
            // serial_println!("One second has passed\n");
        }
    }
}

pub fn timer_wait_sec(seconds: u64) {
    unsafe {
        let ticks = TIMER_TICKS;
        let ticks_to_wait = PIT_HZ * seconds;

        while TIMER_TICKS < ticks + ticks_to_wait {
            // Wacht totdat de juiste hoeveelheid ticks is verstreken
        }

        serial_println!("Time taken: {} ticks", ticks_to_wait);
    }
}

pub fn timer_wait_ms(ms: u64) {
    unsafe {
        let ticks = TIMER_TICKS;

        let ticks_to_wait = ms / 10;

        // Wacht totdat de gewenste hoeveelheid ticks verstreken is
        while TIMER_TICKS < ticks + ticks_to_wait {

        }

        // serial_println!("Time taken: {} ticks", ticks_to_wait);
    }
}