#[cfg(target_os = "macos")]
mod macos;

fn main() {
    #[cfg(target_os = "macos")]
    macos::run();
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!(
            "The native Rust player currently supports macOS. Use the Electron player on Windows."
        );
        std::process::exit(1);
    }
}
