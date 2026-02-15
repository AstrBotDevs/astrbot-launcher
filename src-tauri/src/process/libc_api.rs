//! Unix libc helpers for process management.

use std::fs;
use std::io;
use std::path::PathBuf;

pub fn to_pid_t(pid: u32) -> io::Result<libc::pid_t> {
    i32::try_from(pid)
        .map(|pid| pid as libc::pid_t)
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("pid {pid} out of range"),
            )
        })
}

/// Check if a process exists without sending a signal.
///
/// `kill(pid, 0)` returns:
/// - 0: process exists and we can signal it
/// - EPERM: process exists but we don't have permission to signal it
/// - ESRCH: no such process
pub fn is_process_alive(pid: libc::pid_t) -> bool {
    let rc = unsafe { libc::kill(pid, 0) };
    if rc == 0 {
        true
    } else {
        matches!(io::Error::last_os_error().raw_os_error(), Some(libc::EPERM))
    }
}

pub fn kill(pid: libc::pid_t, sig: libc::c_int) -> io::Result<()> {
    let rc = unsafe { libc::kill(pid, sig) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub fn getpgid(pid: libc::pid_t) -> io::Result<libc::pid_t> {
    let pgid = unsafe { libc::getpgid(pid) };
    if pgid >= 0 {
        Ok(pgid)
    } else {
        Err(io::Error::last_os_error())
    }
}

pub fn killpg(pgid: libc::pid_t, sig: libc::c_int) -> io::Result<()> {
    let rc = unsafe { libc::killpg(pgid, sig) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
pub fn get_process_executable_path(pid: u32) -> Option<PathBuf> {
    let proc_exe = PathBuf::from(format!("/proc/{pid}/exe"));
    let path = fs::read_link(proc_exe).ok()?;

    // Trim garbage after NUL (readlink may include trailing junk).
    let s = path.to_string_lossy();
    let s = s.split('\0').next().unwrap_or("");
    let s = s.strip_suffix(" (deleted)").unwrap_or(s);

    Some(PathBuf::from(s))
}

#[cfg(target_os = "macos")]
pub fn get_process_executable_path(pid: u32) -> Option<PathBuf> {
    let raw_pid = to_pid_t(pid).ok()?;
    match libproc::proc_pid::pidpath(raw_pid) {
        Ok(path) if !path.is_empty() => Some(PathBuf::from(path)),
        _ => None,
    }
}
