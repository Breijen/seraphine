#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(seraphine::test_runner)]
#![reexport_test_harness_main = "test_main"]
#![feature(abi_x86_interrupt)]

extern crate alloc;

use core::panic::PanicInfo;

use bootloader::{BootInfo, entry_point};
use x86_64::VirtAddr;

use seraphine::{println};
use seraphine::print;
use seraphine::memory::{self, BootInfoFrameAllocator};
use seraphine::allocator;

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    println!("Seraphine Control [Version 0.0.1]");
    println!("(c) Seraphine.");
    println!(" ");
    println!("Type 'help' to see available commands.");
    println!(" ");
    seraphine::init();

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe {
        BootInfoFrameAllocator::init(&boot_info.memory_map)
    };

    allocator::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");

    #[cfg(test)]
    test_main();

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