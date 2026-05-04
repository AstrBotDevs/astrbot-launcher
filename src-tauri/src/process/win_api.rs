//! Windows native API helpers for process management.

use std::collections::HashSet;
use std::ffi::c_void;
use std::ffi::OsString;
use std::fmt;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::PathBuf;
use std::sync::Arc;

use windows::core::{Owned, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_MAX_SESSIONS_REACHED, ERROR_MORE_DATA,
    ERROR_SUCCESS, HANDLE, NO_ERROR, STILL_ACTIVE, WIN32_ERROR,
};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID, MIB_TCPROW_OWNER_PID,
    MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::RestartManager::{
    RmEndSession, RmGetList, RmRegisterResources, RmStartSession, CCH_RM_SESSION_KEY,
    RM_PROCESS_INFO,
};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
};

/// Maximum number of retries when the TCP table changes between size query and data fetch.
const TCP_TABLE_MAX_RETRIES: usize = 4;
/// Maximum retries when Restart Manager list changes between calls.
const RM_GET_LIST_MAX_RETRIES: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockingProcessInfo {
    pub pid: u32,
    pub app_name: String,
    pub service_short_name: String,
    pub executable_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct JobObject {
    inner: Arc<Owned<HANDLE>>,
}

// Safety invariants:
// - JobObject only owns launcher-created job handles in this process.
// - The handle is placed in `Owned<HANDLE>` immediately after creation and is
//   never exposed outside this type, so it is closed exactly once when the last
//   Arc clone is dropped.
// - Windows kernel handles are valid across threads in the owning process.
//   The `windows` crate represents HANDLE as a raw pointer and does not mark it
//   Send/Sync, so the process manager needs this wrapper boundary.
unsafe impl Send for JobObject {}
unsafe impl Sync for JobObject {}

impl JobObject {
    /// Create a Windows Job Object that terminates all assigned processes when
    /// the final launcher-owned job handle is closed.
    pub fn create_kill_on_close() -> crate::error::Result<Self> {
        let handle = unsafe { CreateJobObjectW(None, PCWSTR::null()) }.map_err(|e| {
            crate::error::AppError::process(format!("Failed to create job object: {e}"))
        })?;
        let handle = unsafe { Owned::new(handle) };

        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

        unsafe {
            SetInformationJobObject(
                *handle,
                JobObjectExtendedLimitInformation,
                std::ptr::addr_of!(limits).cast::<c_void>(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        }
        .map_err(|e| {
            crate::error::AppError::process(format!(
                "Failed to configure job object kill-on-close: {e}"
            ))
        })?;

        Ok(Self {
            inner: Arc::new(handle),
        })
    }

    /// Assign a process to this job object using the access rights required by
    /// `AssignProcessToJobObject`: PROCESS_SET_QUOTA and PROCESS_TERMINATE.
    pub fn assign_process_by_pid(&self, pid: u32) -> crate::error::Result<()> {
        let process_handle =
            unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid) }.map_err(
                |e| {
                    crate::error::AppError::process(format!(
                        "Failed to open PID {pid} for job object assignment: {e}"
                    ))
                },
            )?;
        let process_handle = unsafe { Owned::new(process_handle) };

        let result =
            unsafe { AssignProcessToJobObject(**self.inner, *process_handle) }.map_err(|e| {
                crate::error::AppError::process(format!(
                    "Failed to assign PID {pid} to job object: {e}"
                ))
            });

        result
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartManagerQueryError {
    StartSession(WIN32_ERROR),
    MaxSessionsReached,
    RegisterResources(WIN32_ERROR),
    GetList(WIN32_ERROR),
    RetryLimitExceeded,
}

impl fmt::Display for RestartManagerQueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StartSession(code) => {
                write!(
                    f,
                    "failed to start Restart Manager session (code={})",
                    code.0
                )
            }
            Self::MaxSessionsReached => write!(
                f,
                "Restart Manager session limit reached; too many concurrent sessions"
            ),
            Self::RegisterResources(code) => {
                write!(
                    f,
                    "failed to register Restart Manager resources (code={})",
                    code.0
                )
            }
            Self::GetList(code) => {
                write!(f, "failed to query Restart Manager list (code={})", code.0)
            }
            Self::RetryLimitExceeded => write!(
                f,
                "Restart Manager list kept changing; retry limit exceeded"
            ),
        }
    }
}

struct RestartManagerSession {
    handle: u32,
}

impl RestartManagerSession {
    fn start() -> std::result::Result<Self, RestartManagerQueryError> {
        let mut handle = 0u32;
        let mut session_key = [0u16; (CCH_RM_SESSION_KEY + 1) as usize];
        let result =
            unsafe { RmStartSession(&mut handle, Some(0), PWSTR(session_key.as_mut_ptr())) };
        if result == ERROR_SUCCESS {
            Ok(Self { handle })
        } else if result == ERROR_MAX_SESSIONS_REACHED {
            Err(RestartManagerQueryError::MaxSessionsReached)
        } else {
            Err(RestartManagerQueryError::StartSession(result))
        }
    }
}

impl Drop for RestartManagerSession {
    fn drop(&mut self) {
        let _ = unsafe { RmEndSession(self.handle) };
    }
}

fn wide_z_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&ch| ch == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

fn get_restart_manager_processes(
    session_handle: u32,
) -> std::result::Result<Vec<RM_PROCESS_INFO>, RestartManagerQueryError> {
    let mut reboot_reasons = 0u32;

    for _ in 0..RM_GET_LIST_MAX_RETRIES {
        let mut needed = 0u32;
        let mut process_count = 0u32;
        let first_result = unsafe {
            RmGetList(
                session_handle,
                &mut needed,
                &mut process_count,
                None,
                &mut reboot_reasons,
            )
        };

        if first_result == ERROR_SUCCESS {
            return Ok(Vec::new());
        }
        if first_result != ERROR_MORE_DATA {
            return Err(RestartManagerQueryError::GetList(first_result));
        }
        if needed == 0 {
            return Ok(Vec::new());
        }

        let mut affected_processes = vec![RM_PROCESS_INFO::default(); needed as usize];
        process_count = needed;
        let second_result = unsafe {
            RmGetList(
                session_handle,
                &mut needed,
                &mut process_count,
                Some(affected_processes.as_mut_ptr()),
                &mut reboot_reasons,
            )
        };

        if second_result == ERROR_SUCCESS {
            affected_processes.truncate(process_count as usize);
            return Ok(affected_processes);
        }
        if second_result != ERROR_MORE_DATA {
            return Err(RestartManagerQueryError::GetList(second_result));
        }
    }

    Err(RestartManagerQueryError::RetryLimitExceeded)
}

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

        match unsafe {
            QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_FORMAT(0),
                PWSTR(path_buf.as_mut_ptr()),
                &mut path_len,
            )
        } {
            Ok(()) => {
                let exe = OsString::from_wide(&path_buf[..path_len as usize]);
                break Some(PathBuf::from(exe));
            }
            Err(e) if e.code() == ERROR_INSUFFICIENT_BUFFER.to_hresult() => {
                capacity = capacity.saturating_mul(2);
                if capacity > 32768 {
                    break None;
                }
            }
            Err(_) => break None,
        }
    };

    let _ = unsafe { CloseHandle(handle) };
    result
}

/// Return processes that currently hold any of the provided files.
///
/// Uses Windows Restart Manager (`RmStartSession` / `RmRegisterResources` /
/// `RmGetList`). Returns an empty vector when no process is holding files.
pub fn get_processes_locking_files(
    file_paths: &[PathBuf],
) -> std::result::Result<Vec<LockingProcessInfo>, RestartManagerQueryError> {
    if file_paths.is_empty() {
        return Ok(Vec::new());
    }

    let session = RestartManagerSession::start()?;

    let wide_paths: Vec<Vec<u16>> = file_paths
        .iter()
        .map(|path| {
            path.as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        })
        .collect();
    let path_ptrs: Vec<PCWSTR> = wide_paths
        .iter()
        .map(|path| PCWSTR(path.as_ptr()))
        .collect();

    let register_result =
        unsafe { RmRegisterResources(session.handle, Some(&path_ptrs), None, None) };
    if register_result != ERROR_SUCCESS {
        return Err(RestartManagerQueryError::RegisterResources(register_result));
    }

    let affected_processes = get_restart_manager_processes(session.handle)?;

    let mut seen_pids = HashSet::new();
    let mut locking_processes = Vec::with_capacity(affected_processes.len());
    for process in affected_processes {
        let pid = process.Process.dwProcessId;
        if pid == 0 || !seen_pids.insert(pid) {
            continue;
        }

        locking_processes.push(LockingProcessInfo {
            pid,
            app_name: wide_z_to_string(&process.strAppName),
            service_short_name: wide_z_to_string(&process.strServiceShortName),
            executable_path: get_process_executable_path(pid),
        });
    }

    Ok(locking_processes)
}
