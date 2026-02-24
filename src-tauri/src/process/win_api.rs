//! Windows native API helpers for process management.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt as _;
use std::path::PathBuf;

use windows::core::PWSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_INSUFFICIENT_BUFFER, NO_ERROR, STILL_ACTIVE,
};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID, MIB_TCPROW_OWNER_PID,
    MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

/// Maximum number of retries when the TCP table changes between size query and data fetch.
const TCP_TABLE_MAX_RETRIES: usize = 4;

/// Fetch the extended TCP table into a properly aligned buffer, retrying on transient
/// `ERROR_INSUFFICIENT_BUFFER` (the table may grow between the size query and the data fetch).
///
/// Returns `None` if the call fails after all retries.
fn fetch_tcp_table(af: u32) -> Option<Vec<u64>> {
    for _ in 0..TCP_TABLE_MAX_RETRIES {
        let mut size: u32 = 0;

        // First call (pTcpTable = NULL): per documentation this always returns
        // ERROR_INSUFFICIENT_BUFFER and fills pdwSize with the required bytes.
        let ret = unsafe {
            GetExtendedTcpTable(None, &mut size, false, af, TCP_TABLE_OWNER_PID_LISTENER, 0)
        };
        if ret != ERROR_INSUFFICIENT_BUFFER.0 || size == 0 {
            return None;
        }

        // Allocate as Vec<u64> to guarantee 8-byte alignment, satisfying the
        // alignment requirement of every MIB_TCP*_OWNER_PID struct field.
        let u64_count = (size as usize).div_ceil(8);
        let mut buffer = vec![0u64; u64_count];

        let ret = unsafe {
            GetExtendedTcpTable(
                Some(buffer.as_mut_ptr().cast()),
                &mut size,
                false,
                af,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };

        if ret == NO_ERROR.0 {
            let actual_u64s = (size as usize).div_ceil(8);
            buffer.truncate(actual_u64s);
            return Some(buffer);
        }

        // Table grew between the two calls — retry.
        if ret != ERROR_INSUFFICIENT_BUFFER.0 {
            return None;
        }
    }
    None
}

/// Search for a listening PID on `port` in the IPv4 TCP table.
fn find_listener_v4(port: u16) -> Option<u32> {
    let buffer = fetch_tcp_table(AF_INET.0 as u32)?;
    let buf_bytes = buffer.len() * 8;

    let table_offset = std::mem::offset_of!(MIB_TCPTABLE_OWNER_PID, table);
    let row_size = std::mem::size_of::<MIB_TCPROW_OWNER_PID>();

    let num_entries = unsafe { std::ptr::read_unaligned(buffer.as_ptr() as *const u32) } as usize;

    for i in 0..num_entries {
        let offset = table_offset.checked_add(i.checked_mul(row_size)?)?;
        if offset.checked_add(row_size)? > buf_bytes {
            break;
        }

        let row = unsafe {
            std::ptr::read_unaligned(
                (buffer.as_ptr() as *const u8).add(offset) as *const MIB_TCPROW_OWNER_PID
            )
        };

        // dwLocalPort is network byte order; mask to lower 16 bits then convert.
        let local_port = u16::from_be((row.dwLocalPort & 0xFFFF) as u16);

        // PID 0 is System Idle Process; never treat it as a valid listener owner.
        if local_port == port && row.dwOwningPid != 0 {
            return Some(row.dwOwningPid);
        }
    }
    None
}

/// Search for a listening PID on `port` in the IPv6 TCP table.
fn find_listener_v6(port: u16) -> Option<u32> {
    let buffer = fetch_tcp_table(AF_INET6.0 as u32)?;
    let buf_bytes = buffer.len() * 8;

    let table_offset = std::mem::offset_of!(MIB_TCP6TABLE_OWNER_PID, table);
    let row_size = std::mem::size_of::<MIB_TCP6ROW_OWNER_PID>();

    let num_entries = unsafe { std::ptr::read_unaligned(buffer.as_ptr() as *const u32) } as usize;

    for i in 0..num_entries {
        let offset = table_offset.checked_add(i.checked_mul(row_size)?)?;
        if offset.checked_add(row_size)? > buf_bytes {
            break;
        }

        let row = unsafe {
            std::ptr::read_unaligned(
                (buffer.as_ptr() as *const u8).add(offset) as *const MIB_TCP6ROW_OWNER_PID
            )
        };

        let local_port = u16::from_be((row.dwLocalPort & 0xFFFF) as u16);

        // PID 0 is System Idle Process; never treat it as a valid listener owner.
        if local_port == port && row.dwOwningPid != 0 {
            return Some(row.dwOwningPid);
        }
    }
    None
}

/// Get PID listening on the given port via Windows API (checks both IPv4 and IPv6).
pub fn get_pid_on_port(port: u16) -> Option<u32> {
    find_listener_v4(port).or_else(|| find_listener_v6(port))
}

/// Check if a process is alive via OpenProcess + GetExitCodeProcess.
pub fn is_process_alive(pid: u32) -> bool {
    unsafe {
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(handle) => {
                let mut exit_code: u32 = 0;
                let alive = GetExitCodeProcess(handle, &mut exit_code).is_ok()
                    && (exit_code as i32) == STILL_ACTIVE.0;
                let _ = CloseHandle(handle);
                alive
            }
            Err(_) => false,
        }
    }
}

/// Resolve executable path for a process via `QueryFullProcessImageNameW`.
pub fn get_process_executable_path(pid: u32) -> Option<PathBuf> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()? };
    let mut capacity = 260u32;

    let result = loop {
        let mut path_buf = vec![0u16; capacity as usize];
        let mut path_len = capacity;

        let ok = unsafe {
            QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_FORMAT(0),
                PWSTR(path_buf.as_mut_ptr()),
                &mut path_len,
            )
            .is_ok()
        };
        if ok {
            let exe = OsString::from_wide(&path_buf[..path_len as usize]);
            break Some(PathBuf::from(exe));
        }

        let last_error = unsafe { GetLastError() };
        if last_error != ERROR_INSUFFICIENT_BUFFER {
            break None;
        }

        capacity = capacity.saturating_mul(2);
        if capacity > 32768 {
            break None;
        }
    };

    let _ = unsafe { CloseHandle(handle) };
    result
}
