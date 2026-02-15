//! Process management utilities.

mod control;
mod health;
mod manager;

#[cfg(unix)]
pub(crate) mod libc_api;

#[cfg(target_os = "windows")]
pub(crate) mod win_api;

use std::time::{Duration, Instant};

use serde::Serialize;

pub use control::{check_port_available, find_available_port, force_kill, graceful_shutdown};
pub use manager::ProcessManager;

/// Grace period before marking instance as disconnected (~2 minutes).
const HEALTH_CHECK_GRACE_PERIOD: Duration = Duration::from_secs(120);

/// Maximum backoff interval between health checks.
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Runtime monitor tick interval.
const MONITOR_INTERVAL: Duration = Duration::from_secs(5);

/// Timeout for graceful shutdown before force killing.
const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeEventReason {
    ProcessTracked,
    ProcessRemoved,
    HealthDisconnected,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeEvent {
    pub instance_id: String,
    pub reason: RuntimeEventReason,
}

/// Information about a running instance.
#[derive(Debug, Clone)]
pub struct InstanceProcess {
    pub pid: u32,
    pub port: u16,
    pub dashboard_enabled: bool,
    /// Whether the original child PID has exited (reported by `child.wait()`).
    pub(crate) pid_exited: bool,
    /// When health check failures started (None if healthy).
    pub(crate) health_failure_since: Option<Instant>,
    /// When to perform the next health check (for exponential backoff).
    pub(crate) next_check_at: Option<Instant>,
    /// Number of consecutive health check failures.
    pub(crate) failure_count: u32,
}

#[derive(Debug, Clone)]
pub struct InstanceRuntimeSnapshot {
    pub running: bool,
    pub port: u16,
    pub dashboard_enabled: bool,
}

impl InstanceProcess {
    pub(crate) fn new(pid: u32, port: u16, dashboard_enabled: bool) -> Self {
        Self {
            pid,
            port,
            dashboard_enabled,
            pid_exited: false,
            health_failure_since: None,
            next_check_at: None,
            failure_count: 0,
        }
    }

    pub(crate) fn calculate_backoff(&self) -> Duration {
        let secs = 1u64 << self.failure_count.min(5); // 1, 2, 4, 8, 16, 32
        Duration::from_secs(secs).min(MAX_BACKOFF)
    }

    pub(crate) fn clear_health_failure_state(&mut self) {
        self.health_failure_since = None;
        self.next_check_at = None;
        self.failure_count = 0;
    }
}
