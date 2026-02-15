//! Unix libc helpers for process management.

use std::io;

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
