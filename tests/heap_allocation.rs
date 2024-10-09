#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(seraphine::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;
use alloc::boxed::Box;
use alloc::format;
use alloc::vec::Vec;
use alloc::string::String;

use bootloader::{entry_point, BootInfo};
use core::panic::PanicInfo;
use seraphine::allocator::HEAP_SIZE;

entry_point!(main);

fn main(boot_info: &'static BootInfo) -> ! {
    use seraphine::allocator;
    use seraphine::memory::{self, BootInfoFrameAllocator};
    use x86_64::VirtAddr;

    seraphine::init();
    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe {
        BootInfoFrameAllocator::init(&boot_info.memory_map)
    };
    allocator::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");

    test_main();
    loop {}
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    seraphine::test_panic_handler(info)
}

#[test_case]
fn simple_allocation() {
    let heap_value_1 = Box::new(41);
    let heap_value_2 = Box::new(13);
    assert_eq!(*heap_value_1, 41);
    assert_eq!(*heap_value_2, 13);
}

#[test_case]
fn large_vec() {
    let n = 1000;
    let mut vec = Vec::new();
    for i in 0..n {
        vec.push(i);
    }
    assert_eq!(vec.iter().sum::<u64>(), (n - 1) * n / 2);
}

#[test_case]
fn many_boxes() {
    for i in 0..HEAP_SIZE {
        let x = Box::new(i);
        assert_eq!(*x, i);
    }
}

#[test_case]
fn strings_allocation() {
    // Test the allocation of multiple strings and ensure their values are correct
    let mut s1 = String::new();
    s1.push_str("Hello");
    assert_eq!(s1, "Hello");

    let mut s2 = String::from("World");
    s2.push_str("!");
    assert_eq!(s2, "World!");

    // Test a large string allocation to see if the heap can handle it
    let large_string_size = 1024; // 1 KiB of characters
    let mut large_string = String::new();
    for _ in 0..large_string_size {
        large_string.push('A');
    }
    assert_eq!(large_string.len(), large_string_size);
    assert!(large_string.chars().all(|c| c == 'A'));

    // Test concatenation of strings
    let concatenated = s1 + " " + &s2; // This consumes `s1`, so we use `+` and a reference to `s2`
    assert_eq!(concatenated, "Hello World!");
}

#[test_case]
fn many_strings() {
    for i in 0..1000 {
        let s = format!("String number {}", i);
        assert_eq!(s, format!("String number {}", i));
    }
}