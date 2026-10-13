#![cfg_attr(windows, windows_subsystem = "windows")]
#[cfg(windows)]
mod space_backend;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
fn main() {
    if let Err(error) = windows::run() {
        windows::show_error(&error);
    }
}
#[cfg(not(windows))]
fn main() {
    eprintln!("The desktop focus adapter currently supports Windows only");
}
