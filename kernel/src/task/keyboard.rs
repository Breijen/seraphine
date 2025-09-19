// Keyboard input is now handled directly in the interrupt handler
// This module is kept minimal for any future keyboard-related utilities

// Note: Direct keyboard processing is implemented in arch/interrupts.rs
// using pc_keyboard crate for scancode decoding and framebuffer for output