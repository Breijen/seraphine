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
use seraphine::task::keyboard;
use seraphine::mem::memory::{self, BootInfoFrameAllocator};
use seraphine::mem::allocator;
use seraphine::filesystem::nvme;
use seraphine::task::{Task};
use seraphine::task::executor::Executor;

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

    //Mapping BIOS
    memory::map_bios_area(&mut mapper, &mut frame_allocator);

    //MAPPING HARD DRIVES
    nvme::init_controller(&mut mapper, &mut frame_allocator);

    // HEAP ALLOCATOR
    allocator::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");

    let mut executor = Executor::new(); // new
    executor.spawn(Task::new(keyboard::print_keypresses()));
    executor.run();

    #[cfg(test)]
    test_main();
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