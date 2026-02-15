//! Platform-agnostic process control functions.

use std::time::{Duration, Instant};

use super::GRACEFUL_SHUTDOWN_TIMEOUT;
use crate::error::{AppError, Result};

/// Check if a process is alive by PID.
#[cfg(target_os = "windows")]
pub fn is_process_alive(pid: u32) -> bool {
    super::win_api::is_process_alive(pid)
}

/// Check if a process is alive by PID.
#[cfg(not(target_os = "windows"))]
pub fn is_process_alive(pid: u32) -> bool {
    let Ok(raw_pid) = super::libc_api::to_pid_t(pid) else {
        return false;
    };
    super::libc_api::is_process_alive(raw_pid)
}

/// Sends CTRL+C via a sidecar helper.
#[cfg(target_os = "windows")]
pub(super) fn graceful_signal(pid: u32) -> Result<()> {
    use std::os::windows::process::CommandExt as _;
    use windows::Win32::System::Threading::CREATE_NO_WINDOW;

    let exe_dir = std::env::current_exe()
        .map_err(|e| AppError::process(format!("Failed to get current exe path: {e}")))?
        .parent()
        .ok_or_else(|| AppError::process("Failed to get exe directory"))?
        .to_path_buf();
    let helper = exe_dir.join("ctrlc_sender.exe");

    std::process::Command::new(&helper)
        .arg(pid.to_string())
        .creation_flags(CREATE_NO_WINDOW.0)
        .spawn()
        .map_err(|e| {
            AppError::process(format!(
                "Failed to spawn ctrlc helper at {}: {e}",
                helper.display()
            ))
        })?;

    Ok(())
}

/// Send a graceful shutdown signal to a process.
#[cfg(not(target_os = "windows"))]
pub(super) fn graceful_signal(pid: u32) -> Result<()> {
    let raw_pid = super::libc_api::to_pid_t(pid)
        .map_err(|e| AppError::process(format!("PID {pid} is out of range for pid_t: {e}")))?;

    super::libc_api::kill(raw_pid, libc::SIGINT)
        .map_err(|e| AppError::process(format!("Failed to send SIGINT to PID {}: {}", pid, e)))
}

#[cfg(target_os = "windows")]
pub fn force_kill(pid: u32) -> Result<()> {
    // Check if process is still alive before attempting to kill
    if !is_process_alive(pid) {
        log::debug!("Process {} is not alive, skipping force kill", pid);
        return Ok(());
    }

    let output = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output()
        .map_err(|e| AppError::process(format!("Failed to run taskkill: {e}")))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = stderr.trim();
        let detail = if detail.is_empty() {
            stdout.trim()
        } else {
            detail
        };
        Err(AppError::process(format!(
            "taskkill failed for pid {}: {}",
            pid,
            if detail.is_empty() {
                "(no output)"
            } else {
                detail
            }
        )))
    }
}

#[cfg(not(target_os = "windows"))]
pub fn force_kill(pid: u32) -> Result<()> {
    // Check if process is still alive before attempting to kill
    if !is_process_alive(pid) {
        log::debug!("Process {} is not alive, skipping force kill", pid);
        return Ok(());
    }

    let target = super::libc_api::to_pid_t(pid)
        .map_err(|e| AppError::process(format!("PID {pid} is out of range for pid_t: {e}")))?;

    match super::libc_api::getpgid(target) {
        Ok(pgid) => super::libc_api::killpg(pgid, libc::SIGKILL).map_err(|e| {
            AppError::process(format!(
                "Failed to kill process group {} (from pid {}): {}",
                pgid, pid, e
            ))
        }),
        Err(e) => super::libc_api::kill(target, libc::SIGKILL).map_err(|kill_err| {
            AppError::process(format!(
                "Failed to kill process {} (getpgid failed: {}): {}",
                pid, e, kill_err
            ))
        }),
    }
}

/// Send graceful signal to each PID, wait up to the timeout for all to exit,
/// then force kill any that remain. Blocking.
pub fn graceful_shutdown(pids: &[u32]) {
    let mut failed_signal_pids = Vec::new();

    for &pid in pids {
        if is_process_alive(pid) {
            if let Err(e) = graceful_signal(pid) {
                log::warn!(
                    "Graceful signal failed for PID {pid}: {e}, will force kill immediately"
                );
                failed_signal_pids.push(pid);
            }
        }
    }

    for &pid in &failed_signal_pids {
        if is_process_alive(pid) {
            if let Err(e) = force_kill(pid) {
                log::error!("Failed to force kill PID {pid}: {e}");
            }
        }
    }

    let successful_pids: Vec<u32> = pids
        .iter()
        .copied()
        .filter(|pid| !failed_signal_pids.contains(pid))
        .collect();

    if successful_pids.is_empty() || successful_pids.iter().all(|&pid| !is_process_alive(pid)) {
        return;
    }

    let deadline = Instant::now() + GRACEFUL_SHUTDOWN_TIMEOUT;
    while Instant::now() < deadline {
        if successful_pids.iter().all(|&pid| !is_process_alive(pid)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    for &pid in &successful_pids {
        if is_process_alive(pid) {
            log::warn!(
                "PID {pid} did not exit within {}s, force killing",
                GRACEFUL_SHUTDOWN_TIMEOUT.as_secs()
            );
            if let Err(e) = force_kill(pid) {
                log::error!("Failed to force kill PID {pid}: {e}");
            }
        }
    }
}

pub fn find_available_port() -> Result<u16> {
    portpicker::pick_unused_port().ok_or_else(|| AppError::process(""))
}

pub fn check_port_available(port: u16) -> Result<()> {
    std::net::TcpListener::bind(("127.0.0.1", port)).map_err(|_| AppError::port_occupied(port))?;
    Ok(())
}
