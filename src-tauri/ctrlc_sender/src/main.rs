#[cfg(windows)]
use std::env;
#[cfg(windows)]
use std::process;

#[cfg(windows)]
use windows::Win32::System::Console::{
    AttachConsole, FreeConsole, GenerateConsoleCtrlEvent, CTRL_C_EVENT,
};

#[cfg(windows)]
fn main() {
    let pid: u32 = env::args()
        .nth(1)
        .unwrap_or_else(|| {
            eprintln!("Usage: ctrlc_sender <pid>");
            process::exit(1);
        })
        .parse()
        .unwrap_or_else(|e| {
            eprintln!("Invalid pid: {e}");
            process::exit(1);
        });

    unsafe {
        let _ = FreeConsole();

        AttachConsole(pid).unwrap_or_else(|e| {
            eprintln!("AttachConsole failed: {e}");
            process::exit(1);
        });

        GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0).unwrap_or_else(|e| {
            eprintln!("GenerateConsoleCtrlEvent failed: {e}");
            process::exit(1);
        });
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("ctrlc_sender is only supported on Windows");
    std::process::exit(1);
}
