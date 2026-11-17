#![cfg_attr(all(target_os = "windows", not(test)), windows_subsystem = "windows")]

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod radio;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;

fn main() {
    #[cfg(target_os = "macos")]
    macos::run();
    #[cfg(target_os = "windows")]
    windows::run();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        eprintln!("The native Rust player supports Windows and macOS.");
        std::process::exit(1);
    }
}
