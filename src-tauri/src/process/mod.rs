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
    graceful_shutdown, resolve_process_executable_path,
};
pub use manager::ProcessManager;

/// Maximum backoff interval between health checks.
const MAX_BACKOFF: Duration = Duration::from_secs(30);

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
#[derive(Debug, Clone)]
pub struct InstanceProcess {
    pub pid: u32,
    pub executable_path: PathBuf,
    pub port: u16,
    pub dashboard_enabled: bool,
    pub state: InstanceState,
    /// When to perform the next health check (for exponential backoff).
    pub(crate) next_health_check_at: Option<std::time::Instant>,
    /// Number of consecutive health check failures.
    pub(crate) health_failure_count: u32,
    /// Number of consecutive failed liveness probes.
    #[cfg(target_os = "windows")]
    pub(crate) alive_failure_count: u32,
    /// When to perform the next liveness probe (for exponential backoff).
    #[cfg(target_os = "windows")]
    pub(crate) next_alive_check_at: Option<std::time::Instant>,
}

#[derive(Debug, Clone)]
pub struct InstanceRuntimeSnapshot {
    pub state: InstanceState,
    pub port: u16,
    pub dashboard_enabled: bool,
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
            state: InstanceState::Starting,
            next_health_check_at: None,
            health_failure_count: 0,
            #[cfg(target_os = "windows")]
            alive_failure_count: 0,
            #[cfg(target_os = "windows")]
            next_alive_check_at: None,
        }
    }

    pub(crate) fn calculate_health_backoff(&self) -> Duration {
        let secs = 1u64 << self.health_failure_count.min(5); // 1, 2, 4, 8, 16, 32
        Duration::from_secs(secs).min(MAX_BACKOFF)
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn calculate_alive_backoff(&self) -> Duration {
        let secs = 1u64 << self.alive_failure_count.min(5); // 1, 2, 4, 8, 16, 32
        Duration::from_secs(secs).min(MAX_BACKOFF)
    }

    pub(crate) fn clear_health_failure_state(&mut self) {
        self.next_health_check_at = None;
        self.health_failure_count = 0;
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn clear_alive_failure_state(&mut self) {
        self.next_alive_check_at = None;
        self.alive_failure_count = 0;
    }
}
