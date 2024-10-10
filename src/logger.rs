#[macro_export]
macro_rules! log {
    ($writer:expr, $($arg:tt)*) => {
        use core::fmt::Write;
        writeln!($writer, "\n[INFO] {}", format_args!($($arg)*)).expect("Failed to write log to VGA buffer");
    };
}

#[macro_export]
macro_rules! err {
    ($writer:expr, $($arg:tt)*) => {
        use core::fmt::Write;
        writeln!($writer, "\n[ERR] {}", format_args!($($arg)*)).expect("Failed to write log to VGA buffer");
    };
}