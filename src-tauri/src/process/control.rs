//! Platform-agnostic process control functions.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::GRACEFUL_SHUTDOWN_TIMEOUT;
use crate::error::{AppError, Result};
#[cfg(target_os = "windows")]
use crate::utils::paths::{get_python_exe_path, get_python_runtime_dir};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecutablePathMatch {
    Match,
    Mismatch,
    Unknown,
}

/// Check if a process is alive by PID.
#[cfg(target_os = "windows")]
pub fn is_process_alive(pid: u32) -> bool {
    super::win_api::is_process_alive(pid)
}

/// Check if a process is alive by PID.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn is_process_alive(pid: u32) -> bool {
    let Ok(raw_pid) = super::libc_api::to_pid_t(pid) else {
        return false;
    };
    super::libc_api::is_process_alive(raw_pid)
}

/// Normalize an executable path before storing or comparing.
pub fn normalize_executable_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Resolve executable path for a process PID and normalize it for comparisons.
pub fn resolve_process_executable_path(pid: u32) -> Option<PathBuf> {
    get_process_executable_path(pid).map(|path| normalize_executable_path(&path))
}

#[cfg(target_os = "windows")]
fn get_process_executable_path(pid: u32) -> Option<PathBuf> {
    super::win_api::get_process_executable_path(pid)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn get_process_executable_path(pid: u32) -> Option<PathBuf> {
    super::libc_api::get_process_executable_path(pid)
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn get_process_executable_path(_pid: u32) -> Option<PathBuf> {
    None
}

#[cfg(target_os = "windows")]
fn normalize_windows_path_for_compare(path: &Path) -> String {
    let value = path.to_string_lossy().replace('/', "\\");
    let value = value.strip_prefix(r"\\?\").unwrap_or(&value);
    value.to_ascii_lowercase()
}

#[cfg(target_os = "windows")]
fn is_python_component_executable(path: &Path) -> bool {
    let path_str = normalize_windows_path_for_compare(path);
    ["py310", "py312"].iter().any(|runtime| {
        let runtime_dir = get_python_runtime_dir(runtime);
        let runtime_exe = get_python_exe_path(&runtime_dir);
        let runtime_exe_str = normalize_windows_path_for_compare(&runtime_exe);
        path_str == runtime_exe_str
    })
}

#[cfg(target_os = "windows")]
fn executable_paths_match(expected: &Path, actual: &Path) -> bool {
    let expected_str = normalize_windows_path_for_compare(expected);
    let actual_str = normalize_windows_path_for_compare(actual);

    if expected_str == actual_str {
        return true;
    }

    // On Windows, venv python.exe is a launcher that may spawn the base Python
    // from the managed Python component runtime (py310/py312).
    if is_python_component_executable(actual) {
        return true;
    }

    false
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn executable_paths_match(expected: &Path, actual: &Path) -> bool {
    expected == actual
}

fn check_executable_path_match(pid: u32, expected_executable_path: &Path) -> ExecutablePathMatch {
    let expected = normalize_executable_path(expected_executable_path);
    let Some(actual) =
        get_process_executable_path(pid).map(|path| normalize_executable_path(&path))
    else {
        return ExecutablePathMatch::Unknown;
    };

    if executable_paths_match(&expected, &actual) {
        ExecutablePathMatch::Match
    } else {
        ExecutablePathMatch::Mismatch
    }
}

/// Monitoring semantics:
/// - `PID alive + Match` => alive
/// - `PID alive + Unknown` => alive (avoid false negatives from transient query failures)
/// - `PID alive + Mismatch` => not alive (possible PID reuse)
pub fn is_expected_process_alive(pid: u32, expected_executable_path: &Path) -> bool {
    if !is_process_alive(pid) {
        return false;
    }

    match check_executable_path_match(pid, expected_executable_path) {
        ExecutablePathMatch::Match => true,
        ExecutablePathMatch::Mismatch => false,
        ExecutablePathMatch::Unknown => {
            log::warn!(
                "Failed to resolve executable path for PID {}, treating as alive for monitoring",
                pid
            );
            true
        }
    }
}

/// Signal semantics:
/// - only `PID alive + Match` is allowed
/// - `Unknown` and `Mismatch` both deny signaling
pub fn can_signal_expected_process(pid: u32, expected_executable_path: &Path) -> bool {
    if !is_process_alive(pid) {
        return false;
    }

    match check_executable_path_match(pid, expected_executable_path) {
        ExecutablePathMatch::Match => true,
        ExecutablePathMatch::Mismatch => false,
        ExecutablePathMatch::Unknown => {
            log::warn!(
                "Failed to resolve executable path for PID {}, skipping signal for safety",
                pid
            );
            false
        }
    }
}

/// Sends CTRL+C via a sidecar helper.
#[cfg(target_os = "windows")]
pub(super) fn graceful_signal(pid: u32) -> Result<()> {
    use std::os::windows::process::CommandExt as _;
    use windows::Win32::System::Threading::CREATE_NO_WINDOW;

    log::debug!("Sending graceful signal to PID {}", pid);

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
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(super) fn graceful_signal(pid: u32) -> Result<()> {
    log::debug!("Sending graceful signal to PID {}", pid);

    let raw_pid = super::libc_api::to_pid_t(pid)
        .map_err(|e| AppError::process(format!("PID {pid} is out of range for pid_t: {e}")))?;

    super::libc_api::kill(raw_pid, libc::SIGINT)
        .map_err(|e| AppError::process(format!("Failed to send SIGINT to PID {}: {}", pid, e)))
}

#[cfg(target_os = "windows")]
pub fn force_kill(pid: u32) -> Result<()> {
    use std::os::windows::process::CommandExt as _;
    use windows::Win32::System::Threading::CREATE_NO_WINDOW;

    // Check if process is still alive before attempting to kill
    if !is_process_alive(pid) {
        log::debug!("Process {} is not alive, skipping force kill", pid);
        return Ok(());
    }
    log::warn!("Force killing PID {}", pid);

    let output = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .creation_flags(CREATE_NO_WINDOW.0)
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

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn force_kill(pid: u32) -> Result<()> {
    // Check if process is still alive before attempting to kill
    if !is_process_alive(pid) {
        log::debug!("Process {} is not alive, skipping force kill", pid);
        return Ok(());
    }
    log::warn!("Force killing PID {}", pid);

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
///
/// Each entry is `(pid, expected_executable_path)`. Identity is verified at
/// every checkpoint so a reused PID is never signalled or killed.
pub fn graceful_shutdown(targets: &[(u32, &Path)]) {
    let mut failed_signal_pids = Vec::new();

    for &(pid, exe) in targets {
        if can_signal_expected_process(pid, exe) {
            if let Err(e) = graceful_signal(pid) {
                log::warn!(
                    "Graceful signal failed for PID {pid}: {e}, will force kill immediately"
                );
                failed_signal_pids.push((pid, exe));
            }
        }
    }

    for &(pid, exe) in &failed_signal_pids {
        if can_signal_expected_process(pid, exe) {
            if let Err(e) = force_kill(pid) {
                log::error!("Failed to force kill PID {pid}: {e}");
            }
        }
    }

    let successful: Vec<(u32, &Path)> = targets
        .iter()
        .copied()
        .filter(|(pid, _)| !failed_signal_pids.iter().any(|(fp, _)| fp == pid))
        .collect();

    if successful.is_empty()
        || successful
            .iter()
            .all(|&(pid, exe)| !is_expected_process_alive(pid, exe))
    {
        return;
    }

    let deadline = Instant::now() + GRACEFUL_SHUTDOWN_TIMEOUT;
    while Instant::now() < deadline {
        if successful
            .iter()
            .all(|&(pid, exe)| !is_expected_process_alive(pid, exe))
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    for &(pid, exe) in &successful {
        if can_signal_expected_process(pid, exe) {
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
    portpicker::pick_unused_port().ok_or_else(|| AppError::process("No available port found"))
}

pub fn check_port_available(port: u16) -> Result<()> {
    std::net::TcpListener::bind(("127.0.0.1", port)).map_err(|_| AppError::port_occupied(port))?;
    Ok(())
}
