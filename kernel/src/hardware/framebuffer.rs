use bootloader_api::info::{FrameBuffer, FrameBufferInfo, PixelFormat};
use core::fmt;
use spin::Mutex;
use lazy_static::lazy_static;
extern crate alloc;
use alloc::string::ToString;

/// Global framebuffer writer instance
lazy_static! {
    pub static ref FRAMEBUFFER: Mutex<Option<FramebufferWriter>> = Mutex::new(None);
}

/// Simple framebuffer writer for UEFI graphics mode
pub struct FramebufferWriter {
    framebuffer: &'static mut [u8],
    info: FrameBufferInfo,
    x_pos: usize,
    y_pos: usize,
    // Cache these values for safety
    max_width: usize,
    max_height: usize,
    buffer_len: usize,
    // Shell functionality
    input_buffer: alloc::string::String,
    prompt_shown: bool,
}

impl FramebufferWriter {
    /// Initialize the framebuffer writer
    pub fn new(framebuffer: &'static mut FrameBuffer) -> Self {
        let info = framebuffer.info();
        let buffer = framebuffer.buffer_mut();
        let buffer_len = buffer.len();

        // Calculate safe bounds
        let max_width = info.width;
        let max_height = info.height;

        Self {
            framebuffer: buffer,
            info,
            x_pos: 0,
            y_pos: 0,
            max_width,
            max_height,
            buffer_len,
            input_buffer: alloc::string::String::new(),
            prompt_shown: false,
        }
    }

    /// Clear the screen with black background (optimized)
    pub fn clear(&mut self) {
        // Fast clear using slice fill (more efficient than loop)
        self.framebuffer.fill(0);
        self.x_pos = 0;
        self.y_pos = 0;
    }

    /// Fill a rectangle with a solid color (OSDev wiki optimization)
    #[allow(dead_code)]
    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, r: u8, g: u8, b: u8) {
        // Bounds checking
        if x >= self.max_width || y >= self.max_height {
            return;
        }

        let end_x = core::cmp::min(x + w, self.max_width);
        let end_y = core::cmp::min(y + h, self.max_height);

        // Calculate color bytes based on pixel format
        let color_bytes = match self.info.pixel_format {
            PixelFormat::Bgr => [b, g, r, 0],
            PixelFormat::Rgb => [r, g, b, 0],
            _ => [b, g, r, 0], // Default to BGR
        };

        // Convert stride from pixels to bytes
        let stride_bytes = self.info.stride * self.info.bytes_per_pixel;

        for row in y..end_y {
            let row_start = row * stride_bytes + x * self.info.bytes_per_pixel;

            for col in x..end_x {
                let pixel_offset = row_start + (col - x) * self.info.bytes_per_pixel;

                if pixel_offset + self.info.bytes_per_pixel <= self.buffer_len {
                    for byte_idx in 0..self.info.bytes_per_pixel {
                        if byte_idx < 4 {
                            self.framebuffer[pixel_offset + byte_idx] = color_bytes[byte_idx];
                        }
                    }
                }
            }
        }
    }

    pub fn write_char(&mut self, c: char) {
        match c {
            '\n' => self.newline(),
            '\r' => self.x_pos = 0,
            c => {
                // Check if we need to wrap to next line
                if self.x_pos + 8 >= self.max_width {
                    self.newline();
                }

                // Only draw if we're in bounds
                if self.y_pos + 8 < self.max_height && self.x_pos + 8 < self.max_width {
                    self.draw_char(c);
                    self.x_pos += 8;
                }
            }
        }
    }

    /// Write a string to the framebuffer
    pub fn write_string(&mut self, s: &str) {
        for c in s.chars() {
            self.write_char(c);
        }
    }

    /// Handle keyboard input for shell functionality
    pub fn handle_keyboard_input(&mut self, c: char) {
        match c {
            '\n' => {
                self.write_char('\n');
                self.execute_command();
                self.show_prompt();
            }
            '\u{8}' => {
                // Backspace
                if !self.input_buffer.is_empty() {
                    self.input_buffer.pop();
                    self.backspace();
                }
            }
            c if c.is_ascii_graphic() || c == ' ' => {
                self.input_buffer.push(c);
                self.write_char(c);
            }
            _ => {
                // Ignore non-printable characters
            }
        }
    }

    /// Show the shell prompt
    pub fn show_prompt(&mut self) {
        if !self.prompt_shown {
            self.write_string("seraphine> ");
            self.prompt_shown = true;
        }
    }

    /// Handle backspace by moving cursor back and clearing character
    fn backspace(&mut self) {
        if self.x_pos >= 8 {
            self.x_pos -= 8;
            // Draw space over the character to erase it
            self.draw_char(' ');
            self.x_pos -= 8; // Move back again since draw_char advances
        }
    }

    /// Execute a shell command
    fn execute_command(&mut self) {
        // Clone the command to avoid borrow checker issues
        let command = self.input_buffer.trim().to_string();

        if command.is_empty() {
            self.input_buffer.clear();
            self.prompt_shown = false;
            return;
        }

        let mut parts = command.split_whitespace();
        let command_name = parts.next().unwrap_or("").to_string();
        let arguments: alloc::vec::Vec<alloc::string::String> = parts.map(|s| s.to_string()).collect();

        match command_name.as_str() {
            "help" => {
                self.write_string("\nAvailable commands:\n");
                self.write_string("  help  - Show this help message\n");
                self.write_string("  clear - Clear the screen\n");
                self.write_string("  echo  - Echo the input text\n");
                self.write_string("  info  - Show system information\n");
            }
            "clear" => {
                self.clear();
            }
            "echo" => {
                self.write_char('\n');
                for (i, arg) in arguments.iter().enumerate() {
                    if i > 0 {
                        self.write_char(' ');
                    }
                    self.write_string(arg);
                }
                self.write_char('\n');
            }
            "info" => {
                self.write_string("\nSeraphine OS - UEFI Framebuffer Mode\n");
                self.write_string(&alloc::format!("Resolution: {}x{}\n", self.max_width, self.max_height));
                self.write_string(&alloc::format!("Pixel format: {:?}\n", self.info.pixel_format));
                self.write_string(&alloc::format!("Bytes per pixel: {}\n", self.info.bytes_per_pixel));
            }
            "" => {
                // Empty command, just show new prompt
            }
            _ => {
                self.write_string("\nUnknown command: ");
                self.write_string(&command);
                self.write_string("\nType 'help' to see available commands.\n");
            }
        }

        self.input_buffer.clear();
        self.prompt_shown = false;
    }

    /// Move to a new line
    fn newline(&mut self) {
        self.y_pos += 16; // 16 pixel line height
        self.x_pos = 0;

        // Clear screen if we're too close to bottom
        if self.y_pos + 16 >= self.max_height {
            self.clear();
        }
    }

    /// Draw a character using optimized pixel access (based on OSDev wiki)
    fn draw_char(&mut self, c: char) {
        let font_data = *self.get_font_data(c);

        // Bounds check for the entire character
        if self.x_pos + 8 > self.max_width || self.y_pos + 8 > self.max_height {
            return;
        }

        // Convert stride from pixels to bytes (bootloader reports stride in pixels)
        let stride_bytes = self.info.stride * self.info.bytes_per_pixel;

        // Calculate base offset once for the character
        let base_y_offset = self.y_pos * stride_bytes;
        let base_x_offset = self.x_pos * self.info.bytes_per_pixel;

        for (row, bitmap) in font_data.iter().enumerate() {
            // Calculate row offset once per row
            let row_offset = base_y_offset + row * stride_bytes + base_x_offset;

            // Check if this row is within bounds
            if row_offset + 8 * self.info.bytes_per_pixel > self.buffer_len {
                break;
            }

            for col in 0..8 {
                if (bitmap >> (7 - col)) & 1 == 1 {
                    // Direct pixel access without function call overhead
                    let pixel_offset = row_offset + col * self.info.bytes_per_pixel;

                    // Write white pixel (255, 255, 255) in BGRA format
                    match self.info.pixel_format {
                        PixelFormat::Bgr => {
                            self.framebuffer[pixel_offset] = 255;     // B
                            self.framebuffer[pixel_offset + 1] = 255; // G
                            self.framebuffer[pixel_offset + 2] = 255; // R
                            if self.info.bytes_per_pixel == 4 {
                                self.framebuffer[pixel_offset + 3] = 0; // A
                            }
                        }
                        _ => {
                            // Fallback for other formats
                            self.framebuffer[pixel_offset] = 255;
                            if pixel_offset + 1 < self.buffer_len {
                                self.framebuffer[pixel_offset + 1] = 255;
                            }
                            if pixel_offset + 2 < self.buffer_len {
                                self.framebuffer[pixel_offset + 2] = 255;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Get simple 8x8 bitmap font data for a character
    fn get_font_data(&self, c: char) -> &[u8; 8] {
        match c {
            'A' => &[0x18, 0x24, 0x42, 0x42, 0x7E, 0x42, 0x42, 0x42],
            'B' => &[0x7C, 0x42, 0x42, 0x7C, 0x42, 0x42, 0x42, 0x7C],
            'C' => &[0x3C, 0x42, 0x40, 0x40, 0x40, 0x40, 0x42, 0x3C],
            'D' => &[0x78, 0x44, 0x42, 0x42, 0x42, 0x42, 0x44, 0x78],
            'E' => &[0x7E, 0x40, 0x40, 0x7C, 0x40, 0x40, 0x40, 0x7E],
            'F' => &[0x7E, 0x40, 0x40, 0x7C, 0x40, 0x40, 0x40, 0x40],
            'G' => &[0x3C, 0x42, 0x40, 0x4E, 0x42, 0x42, 0x42, 0x3C],
            'H' => &[0x42, 0x42, 0x42, 0x7E, 0x42, 0x42, 0x42, 0x42],
            'I' => &[0x3E, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x3E],
            'J' => &[0x02, 0x02, 0x02, 0x02, 0x02, 0x42, 0x42, 0x3C],
            'K' => &[0x44, 0x48, 0x50, 0x60, 0x50, 0x48, 0x44, 0x42],
            'L' => &[0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x7E],
            'M' => &[0x42, 0x66, 0x5A, 0x42, 0x42, 0x42, 0x42, 0x42],
            'N' => &[0x42, 0x62, 0x52, 0x4A, 0x46, 0x42, 0x42, 0x42],
            'O' => &[0x3C, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x3C],
            'P' => &[0x7C, 0x42, 0x42, 0x7C, 0x40, 0x40, 0x40, 0x40],
            'Q' => &[0x3C, 0x42, 0x42, 0x42, 0x4A, 0x44, 0x3A, 0x00],
            'R' => &[0x7C, 0x42, 0x42, 0x7C, 0x48, 0x44, 0x42, 0x42],
            'S' => &[0x3C, 0x42, 0x40, 0x3C, 0x02, 0x02, 0x42, 0x3C],
            'T' => &[0x7F, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08],
            'U' => &[0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x3C],
            'V' => &[0x42, 0x42, 0x42, 0x42, 0x42, 0x24, 0x18, 0x00],
            'W' => &[0x42, 0x42, 0x42, 0x42, 0x42, 0x5A, 0x66, 0x42],
            'X' => &[0x42, 0x24, 0x18, 0x00, 0x18, 0x24, 0x42, 0x42],
            'Y' => &[0x41, 0x22, 0x14, 0x08, 0x08, 0x08, 0x08, 0x08],
            'Z' => &[0x7E, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x7E],
            'a'..='z' => {
                // Simple mapping: lowercase = uppercase
                let upper = ((c as u8) - b'a' + b'A') as char;
                return self.get_font_data(upper);
            }
            '0' => &[0x3C, 0x46, 0x4A, 0x52, 0x62, 0x42, 0x42, 0x3C],
            '1' => &[0x18, 0x28, 0x08, 0x08, 0x08, 0x08, 0x08, 0x3E],
            '2' => &[0x3C, 0x42, 0x02, 0x0C, 0x30, 0x40, 0x40, 0x7E],
            '3' => &[0x3C, 0x42, 0x02, 0x1C, 0x02, 0x02, 0x42, 0x3C],
            '4' => &[0x04, 0x0C, 0x14, 0x24, 0x44, 0x7E, 0x04, 0x04],
            '5' => &[0x7E, 0x40, 0x40, 0x7C, 0x02, 0x02, 0x42, 0x3C],
            '6' => &[0x3C, 0x42, 0x40, 0x7C, 0x42, 0x42, 0x42, 0x3C],
            '7' => &[0x7E, 0x02, 0x04, 0x08, 0x10, 0x20, 0x20, 0x20],
            '8' => &[0x3C, 0x42, 0x42, 0x3C, 0x42, 0x42, 0x42, 0x3C],
            '9' => &[0x3C, 0x42, 0x42, 0x42, 0x3E, 0x02, 0x42, 0x3C],
            ' ' => &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
            '.' => &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18],
            '!' => &[0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x18, 0x18],
            ':' => &[0x00, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x00],
            '(' => &[0x04, 0x08, 0x10, 0x10, 0x10, 0x10, 0x08, 0x04],
            ')' => &[0x20, 0x10, 0x08, 0x08, 0x08, 0x08, 0x10, 0x20],
            '[' => &[0x1C, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1C],
            ']' => &[0x38, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x38],
            '\'' => &[0x18, 0x18, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00],
            '>' => &[0x00, 0x40, 0x20, 0x10, 0x20, 0x40, 0x00, 0x00], // Greater than symbol
            _ => &[0xFF, 0x81, 0x81, 0x81, 0x81, 0x81, 0x81, 0xFF], // Unknown char box
        }
    }

    /// Set a pixel using the OSDev wiki formula: pixel = vram + y*pitch + x*pixelwidth
    fn set_pixel(&mut self, x: usize, y: usize, r: u8, g: u8, b: u8) {
        // Bounds checking
        if x >= self.max_width || y >= self.max_height {
            return;
        }

        // Convert stride from pixels to bytes (bootloader reports stride in pixels)
        let stride_bytes = self.info.stride * self.info.bytes_per_pixel;
        // OSDev wiki formula: unsigned char *pixel = vram + y*pitch + x*pixelwidth;
        let pixel_offset = y * stride_bytes + x * self.info.bytes_per_pixel;

        // Safety check
        if pixel_offset + self.info.bytes_per_pixel > self.buffer_len {
            return;
        }

        // Write pixel based on format - optimized for common case (32-bit BGRA)
        match self.info.pixel_format {
            PixelFormat::Bgr => {
                // BGRA format (most common in UEFI)
                self.framebuffer[pixel_offset] = b;
                self.framebuffer[pixel_offset + 1] = g;
                self.framebuffer[pixel_offset + 2] = r;
                if self.info.bytes_per_pixel == 4 {
                    self.framebuffer[pixel_offset + 3] = 0; // Alpha
                }
            }
            PixelFormat::Rgb => {
                // RGBA format
                self.framebuffer[pixel_offset] = r;
                self.framebuffer[pixel_offset + 1] = g;
                self.framebuffer[pixel_offset + 2] = b;
                if self.info.bytes_per_pixel == 4 {
                    self.framebuffer[pixel_offset + 3] = 0; // Alpha
                }
            }
            PixelFormat::U8 => {
                // 8-bit grayscale
                let gray = ((r as u16 + g as u16 + b as u16) / 3) as u8;
                self.framebuffer[pixel_offset] = gray;
            }
            _ => {
                // Default to BGR for unknown formats
                self.framebuffer[pixel_offset] = b;
                if pixel_offset + 1 < self.buffer_len {
                    self.framebuffer[pixel_offset + 1] = g;
                }
                if pixel_offset + 2 < self.buffer_len {
                    self.framebuffer[pixel_offset + 2] = r;
                }
            }
        }
    }
}

impl fmt::Write for FramebufferWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

/// Initialize the global framebuffer
pub fn init_framebuffer(framebuffer: &'static mut FrameBuffer) {
    let info = framebuffer.info();

    // Debug output to serial
    use crate::serial_println;
    serial_println!("Framebuffer info:");
    serial_println!("  Width: {}", info.width);
    serial_println!("  Height: {}", info.height);
    serial_println!("  Stride: {} pixels", info.stride);
    serial_println!("  Stride: {} bytes", info.stride * info.bytes_per_pixel);
    serial_println!("  Bytes per pixel: {}", info.bytes_per_pixel);
    serial_println!("  Pixel format: {:?}", info.pixel_format);
    serial_println!("  Buffer length: {}", framebuffer.buffer().len());
    serial_println!("  Expected size: {}", info.height * info.stride * info.bytes_per_pixel);

    let mut writer = FramebufferWriter::new(framebuffer);
    writer.clear();
    *FRAMEBUFFER.lock() = Some(writer);
}

/// Print to the framebuffer (internal function)
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;

    if let Some(ref mut writer) = FRAMEBUFFER.lock().as_mut() {
        writer.write_fmt(args).unwrap();
    }
}

/// Handle keyboard input for the framebuffer shell
pub fn handle_keyboard_char(c: char) {
    if let Some(ref mut writer) = FRAMEBUFFER.lock().as_mut() {
        writer.handle_keyboard_input(c);
    }
}

/// Show the initial shell prompt
pub fn show_initial_prompt() {
    if let Some(ref mut writer) = FRAMEBUFFER.lock().as_mut() {
        writer.show_prompt();
    }
}

/// Print macro for framebuffer
#[macro_export]
macro_rules! fb_print {
    ($($arg:tt)*) => {
        $crate::hardware::framebuffer::_print(format_args!($($arg)*))
    };
}

/// Println macro for framebuffer
#[macro_export]
macro_rules! fb_println {
    () => ($crate::fb_print!("\n"));
    ($($arg:tt)*) => ($crate::fb_print!("{}\n", format_args!($($arg)*)));
}