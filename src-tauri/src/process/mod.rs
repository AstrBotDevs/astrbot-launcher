//! Process management utilities.

mod control;
mod health;
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

/// Maximum backoff interval between health checks.
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Capped exponential backoff: `2^count` seconds, clamped to [`MAX_BACKOFF`].
///
/// Shared by health checks and liveness probes so the formula stays in one
/// place.  Callers can wrap this if they ever need independent tuning.
pub(super) fn calculate_backoff(failure_count: u32) -> Duration {
    let secs = 1u64 << failure_count.min(5); // 1, 2, 4, 8, 16, 32
    Duration::from_secs(secs).min(MAX_BACKOFF)
}

/// Runtime monitor tick interval.
const MONITOR_INTERVAL: Duration = Duration::from_secs(5);

/// Timeout for graceful shutdown before force killing.
const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(60);

/// Number of consecutive health check failures before marking as unhealthy.
const UNHEALTHY_THRESHOLD: u32 = 3;

/// On Windows, number of consecutive liveness probe failures before treating
/// process-alive as definitively false.
#[cfg(target_os = "windows")]
const ALIVE_EXIT_THRESHOLD: u32 = 5;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstanceState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Unhealthy,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeEvent {
    pub instance_id: String,
    pub state: InstanceState,
}

/// Information about a running instance.
///
/// The instance's health state (Running vs Unhealthy) is derived from
/// `health_failure_count` rather than stored explicitly — see
/// [`manager::derive_health_state`].
#[derive(Debug, Clone)]
pub struct InstanceProcess {
    pub pid: u32,
    pub executable_path: PathBuf,
    pub port: u16,
    pub dashboard_enabled: bool,
    /// When to perform the next health check (for exponential backoff).
    pub(crate) next_health_check_at: Option<std::time::Instant>,
    /// Number of consecutive health check failures.
    pub(crate) health_failure_count: u32,
    /// Number of consecutive failed liveness probes.
    /// Always 0 on non-Windows (no retry mechanism).
    pub(crate) alive_failure_count: u32,
    /// When to perform the next liveness probe (for exponential backoff).
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
    /// Process running. Health state derived from `InstanceProcess`.
    Live {
        state: InstanceState,
        port: u16,
        dashboard_enabled: bool,
    },
    /// Shutdown in progress. Values captured from Live at transition time.
    Stopping {
        port: u16,
        dashboard_enabled: bool,
    },
}

impl InstanceProcess {
    pub(crate) fn new(
        pid: u32,
        executable_path: PathBuf,
        port: u16,
        dashboard_enabled: bool,
    ) -> Self {
        Self {
            pid,
            executable_path,
            port,
            dashboard_enabled,
            next_health_check_at: None,
            health_failure_count: 0,
            alive_failure_count: 0,
            next_alive_check_at: None,
        }
    }
}
