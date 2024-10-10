use core::fmt;
use core::fmt::Write;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;

use lazy_static::lazy_static;
use volatile::Volatile;
use x86_64::instructions::interrupts;
use crate::{hardware, serial_println};

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct ColorCode(u8);

impl ColorCode {
    fn new(foreground: Color, background: Color) -> ColorCode {
        ColorCode((background as u8) << 4 | (foreground as u8))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct ScreenChar {
    ascii_character: u8,
    color_code: ColorCode,
}

const BUFFER_HEIGHT: usize = 25;
const BUFFER_WIDTH: usize = 80;

#[repr(transparent)]
struct Buffer {
    chars: [[Volatile<ScreenChar>; BUFFER_WIDTH]; BUFFER_HEIGHT],
}

pub struct Writer {
    prompt_position: usize,
    cursor_position: usize,
    input_buffer: String,
    color_code: ColorCode,
    buffer: &'static mut Buffer,
    user_input_mode: bool,
}

lazy_static! {
    pub static ref WRITER: Mutex<Writer> = Mutex::new(Writer {
        prompt_position: 1,
        cursor_position: 3,
        input_buffer: String::new(),
        color_code: ColorCode::new(Color::Red, Color::Black),
        buffer: unsafe { &mut *(0xb8000 as *mut Buffer) },
        user_input_mode: false,
    });
}

impl Writer {
    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => {
                if self.user_input_mode {
                    self.user_input_mode = false;
                    self.execute_command();
                }
                self.new_line();
            }
            b'\x08' => {
                if self.cursor_position > self.prompt_position + 2 {
                    self.move_cursor_left();
                    let row = BUFFER_HEIGHT - 1;
                    let col = self.cursor_position;

                    // Wis het karakter van het scherm door een spatie te schrijven
                    self.buffer.chars[row][col].write(ScreenChar {
                        ascii_character: b' ',
                        color_code: self.color_code,
                    });

                    // Verwijder het laatste karakter van de invoerbuffer, indien we in gebruikersmodus zijn
                    if self.user_input_mode && !self.input_buffer.is_empty() {
                        self.input_buffer.pop();
                    }
                }
            }
            byte => {

                if self.cursor_position >= BUFFER_WIDTH {
                    self.new_line();
                }

                let row = BUFFER_HEIGHT - 1;
                let col = self.cursor_position;

                let color_code = self.color_code;
                self.buffer.chars[row][col].write(ScreenChar {
                    ascii_character: byte,
                    color_code,
                });

                // Alleen toevoegen aan de input buffer als we in de gebruikersinvoer-modus zijn
                if self.user_input_mode {
                    self.input_buffer.push(byte as char);
                }

                self.cursor_position += 1;
            }
        }

    }

    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            match byte {
                // ASCII byte or newline
                0x20..=0x7e | b'\n' => self.write_byte(byte),
                _ => self.write_byte(0xfe),
            }
        }
    }

    fn new_line(&mut self) {
        for row in 1..BUFFER_HEIGHT {
            for col in 0..BUFFER_WIDTH {
                let character = self.buffer.chars[row][col].read();
                self.buffer.chars[row - 1][col].write(character);
            }
        }

        self.clear_row(BUFFER_HEIGHT - 1);
        self.cursor_position = 3;
        self.input_buffer.clear();
    }

    pub fn toggle_prompt(&mut self, visible: bool) {
        let row = BUFFER_HEIGHT - 1;
        let col = self.prompt_position;

        let color_code = self.color_code;
        let ascii_character = if visible { b'>' } else { b' ' };

        self.buffer.chars[row][col].write(ScreenChar {
            ascii_character,
            color_code,
        });

        self.user_input_mode = true;
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor_position > self.prompt_position {
            self.cursor_position -= 1;
        }
    }
}

impl Writer {
    pub fn execute_command(&mut self) {
        let command = self.input_buffer.trim().to_string();

        let mut parts = command.split_whitespace();
        let command_name = parts.next().unwrap_or("");
        let arguments: Vec<&str> = parts.collect();

        match command_name {
            "help" => {
                self.write_string("\n");
                self.write_string("\nAvailable commands:\n");
                self.write_string("help  - Show this help message\n");
                self.write_string("clear - Clear the screen\n");
                self.write_string("echo  - Echo the input text\n");
            }
            "clear" => {
                self.clear_screen();
            }
            "echo" => {
                self.write_string("\n");
                for arg in arguments {
                    self.write_string(arg);
                    self.write_string(" ");
                }
                self.write_string("\n");
            }
            _ => {
                self.write_string("\nUnknown command: ");
                self.write_string(&command);
                self.write_string("\nType 'help' to see available commands.\n");
            }
        }

        self.input_buffer.clear();
    }

    fn clear_row(&mut self, row: usize) {
        let blank = ScreenChar {
            ascii_character: b' ',
            color_code: self.color_code,
        };
        for col in 0..BUFFER_WIDTH {
            self.buffer.chars[row][col].write(blank);
        }
    }

    pub fn clear_screen(&mut self) {
        for row in 0..BUFFER_HEIGHT {
            self.clear_row(row);
        }
        self.cursor_position = self.prompt_position + 2; // Reset cursorpositie na de prompt
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

// ----------------------------------------------------------------------------------------
// Macros

#[macro_export]
macro_rules! println {
    () => (print!("\n"));
    ($($arg:tt)*) => (print!("{}\n", format_args!($($arg)*)));
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::vga_buffer::_print(format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;

    interrupts::without_interrupts(|| {
        WRITER.lock().write_fmt(args).unwrap();
    });
}

// ----------------------------------------------------------------------------------------
// Tests

#[test_case]
fn test_println() {
    println!("test_println output")
}

#[test_case]
fn test_println_output() {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;

    let s = "Some test string that fits on a single line";
    interrupts::without_interrupts(|| {
        let mut writer = WRITER.lock();
        writeln!(writer, "\n{}", s).expect("writeln failed");
        for (i, c) in s.chars().enumerate() {
            let screen_char = writer.buffer.chars[BUFFER_HEIGHT - 2][i + writer.prompt_position + 2].read();
            assert_eq!(char::from(screen_char.ascii_character), c);
        }
    });
}