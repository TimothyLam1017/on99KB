#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod app;

fn main() {
    #[cfg(windows)]
    app::run_or_report();

    #[cfg(not(windows))]
    {
        eprintln!("on99KB is a Windows application.");
        eprintln!("Cross-compile the exe from Linux with:");
        eprintln!("  rustup target add x86_64-pc-windows-gnu");
        eprintln!("  cargo build --release --target x86_64-pc-windows-gnu");
        eprintln!("The executable is then at:");
        eprintln!("  target/x86_64-pc-windows-gnu/release/on99KB.exe");
        std::process::exit(1);
    }
}
