#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(seraphine::test_runner)]
#![reexport_test_harness_main = "test_main"]
#![feature(abi_x86_interrupt)]

use core::panic::PanicInfo;
use seraphine::println;
use seraphine::print;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    println!("Hello World{}", "!");

    seraphine::init();

    #[cfg(test)]
    test_main();

    println!("It did not crash!");
    seraphine::hlt_loop();
}

/// This function is called on panic.
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    seraphine::hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    seraphine::test_panic_handler(info)
}