//! Process management utilities.

mod control;
mod manager;
mod monitor;

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) mod libc_api;

#[cfg(target_os = "windows")]
pub(crate) mod win_api;

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub use control::{
    can_signal_expected_process, check_port_available, find_available_port, force_kill,
    graceful_shutdown, is_expected_process_alive, resolve_process_executable_path,
};
pub use manager::ProcessManager;
#[cfg(target_os = "windows")]
pub(crate) use win_api::JobObject;

/// Timeout (in seconds) for waiting for the startup log message.
pub const STARTUP_TIMEOUT_SECS: u64 = 300;

/// Timeout (in seconds) for liveness probes. After this many seconds of
/// consecutive probe failures, the process is treated as dead.
pub const LIVENESS_TIMEOUT_SECS: u64 = 300;

/// Liveness probe interval in seconds.
const LIVENESS_PROBE_INTERVAL_SECS: u64 = 5;

/// Runtime monitor tick interval (also the fixed liveness probe interval).
const MONITOR_INTERVAL: Duration = Duration::from_secs(LIVENESS_PROBE_INTERVAL_SECS);

/// Timeout for graceful shutdown before force killing.
const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(60);

/// On Windows, number of consecutive liveness probe failures before treating
/// process-alive as definitively false.
///
/// Computed as `ceil(LIVENESS_TIMEOUT_SECS / LIVENESS_PROBE_INTERVAL_SECS)` so
/// the total tolerance window is at least [`LIVENESS_TIMEOUT_SECS`].
#[cfg(target_os = "windows")]
const ALIVE_EXIT_THRESHOLD: u32 =
    LIVENESS_TIMEOUT_SECS.div_ceil(LIVENESS_PROBE_INTERVAL_SECS) as u32;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstanceState {
    Stopped,
    Starting,
    Running,
    Stopping,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeEvent {
    pub instance_id: String,
    pub state: InstanceState,
}

/// Information about a running instance.
#[derive(Debug, Clone)]
pub struct InstanceProcess {
    pub pid: u32,
    pub executable_path: PathBuf,
    pub port: u16,
    pub dashboard_enabled: bool,
    #[cfg(target_os = "windows")]
    pub(crate) _job_object: JobObject,
    /// Number of consecutive failed liveness probes.
    /// Always 0 on non-Windows (no retry mechanism).
    pub(crate) alive_failure_count: u32,
    /// When to perform the next liveness probe (fixed interval).
    /// Always `None` on non-Windows (no retry mechanism).
    pub(crate) next_alive_check_at: Option<std::time::Instant>,
}

/// Typed runtime info returned by the process manager.
///
/// Each variant carries only data the ProcessManager uniquely owns.
/// Config-derived fields (configured port, dashboard_enabled when stopped)
/// are read from their sources of truth at snapshot assembly time.
#[derive(Debug, Clone)]
pub enum InstanceRuntimeInfo {
    /// Launch in progress — config-derived fields are read from their
    /// sources of truth at snapshot assembly time.
    Starting,
    /// Process running.
    Live { port: u16, dashboard_enabled: bool },
    /// Shutdown in progress. Values captured from Live at transition time.
    Stopping { port: u16, dashboard_enabled: bool },
}

impl InstanceProcess {
    pub(crate) fn new(
        pid: u32,
        executable_path: PathBuf,
        port: u16,
        dashboard_enabled: bool,
        #[cfg(target_os = "windows")] job_object: JobObject,
    ) -> Self {
        Self {
            pid,
            executable_path,
            port,
            dashboard_enabled,
            #[cfg(target_os = "windows")]
            _job_object: job_object,
            alive_failure_count: 0,
            next_alive_check_at: None,
        }
    }
}
