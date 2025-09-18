#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(seraphine_kernel::test_runner)]
#![reexport_test_harness_main = "test_main"]
#![feature(abi_x86_interrupt)]

extern crate alloc;

use core::panic::PanicInfo;

use bootloader_api::{BootInfo, entry_point};
use x86_64::VirtAddr;



use bootloader_api::{BootloaderConfig};
use seraphine_kernel::{fb_println, println, print};

const CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();

    // UEFI-compatible configuration with identity mapping fallback support
    // This configuration works whether the bootloader provides physical_memory_offset or not

    // Use dynamic mapping for boot info and framebuffer to avoid conflicts
    config.mappings.boot_info = bootloader_api::config::Mapping::Dynamic;
    config.mappings.framebuffer = bootloader_api::config::Mapping::Dynamic;

    // Explicitly request physical memory mapping for better compatibility
    // If bootloader can't provide it, we'll fall back to identity mapping
    config.mappings.physical_memory = Some(bootloader_api::config::Mapping::Dynamic);

    // Conservative stack size for stability (64KB matches our rustflags)
    config.kernel_stack_size = 65536;

    config
};

entry_point!(kernel_main, config = &CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    use seraphine_kernel::{
        serial::init_serial,
        serial_println,
        mem::memory::{self, BootInfoFrameAllocator},
        mem::allocator,
        hardware::framebuffer,
        arch::{gdt, interrupts},
        hardware::vga,
    };
    use bootloader_api::info::{MemoryRegionKind, Optional};
    use x86_64::VirtAddr;

    // === Phase 0: Basic Debug Output ===
    let _ = init_serial(); // Ignore failure silently in release builds

    serial_println!("Seraphine Kernel Boot (UEFI)");
    serial_println!("Target Arch: x86_64");

    // === Phase 1: CPU Initialization ===
    gdt::init();
    interrupts::init_idt();
    vga::disable_hardware_cursor();
    unsafe { interrupts::PICS.lock().initialize(); }
    x86_64::instructions::interrupts::enable();

    // === Phase 2: Memory Setup ===

    // Step 2.1: Determine physical memory offset
    let phys_mem_offset = match boot_info.physical_memory_offset {
        Optional::Some(offset) if is_canonical(offset) && offset != 0 => VirtAddr::new(offset),
        _ => VirtAddr::new(0), // fallback: identity mapping
    };

    // Step 2.2: Check usable memory regions
    let memory_regions = &boot_info.memory_regions;
    let mut usable_mem = 0;
    let mut found_usable = false;

    for region in memory_regions.iter() {
        if region.kind == MemoryRegionKind::Usable {
            usable_mem += region.end - region.start;
            found_usable = true;
        }
    }

    if !found_usable {
        panic!("No usable memory regions found.");
    }

    // Step 2.3: Initialize mapper & frame allocator
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(memory_regions) };

    // Step 2.4: Initialize heap
    allocator::init_heap(&mut mapper, &mut frame_allocator, memory_regions)
        .expect("Heap initialization failed");

    // === Phase 3: Graphics ===
    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        let info = framebuffer.info();

        match info.pixel_format {
            bootloader_api::info::PixelFormat::Rgb
            | bootloader_api::info::PixelFormat::Bgr => {
                framebuffer::init_framebuffer(framebuffer);

                fb_println!("Seraphine OS - UEFI Boot Complete");
                fb_println!("Resolution: {}x{}", info.width, info.height);
                fb_println!("Memory: {}MB", usable_mem / 1024 / 1024);
                fb_println!("System Ready");
            }
            _ => {
                serial_println!("Unsupported framebuffer pixel format. Using VGA fallback.");
            }
        }
    } else {
        println!("Seraphine OS Boot Complete");
        println!("Graphics: VGA Text Mode");
    }

    serial_println!("System initialized. Entering idle loop...");

    loop {
        x86_64::instructions::hlt();
    }
}

fn is_canonical(addr: u64) -> bool {
    let bit47 = (addr >> 47) & 1;
    let upper = addr >> 48;
    (bit47 == 0 && upper == 0) || (bit47 == 1 && upper == 0xFFFF)
}

/// This function is called on panic.
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    seraphine_kernel::hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    seraphine_kernel::test_panic_handler(info)
}