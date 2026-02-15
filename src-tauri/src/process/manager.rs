//! Instance process tracking and runtime monitoring.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, Instant};

use reqwest::Client;
use tokio::sync::broadcast;

use super::control::{can_signal_expected_process, graceful_shutdown, is_expected_process_alive};
use super::health::check_health;
#[cfg(target_os = "windows")]
use super::win_api::get_pid_on_port;
use super::{
    InstanceProcess, InstanceRuntimeSnapshot, RuntimeEvent, RuntimeEventReason,
    HEALTH_CHECK_GRACE_PERIOD, MONITOR_INTERVAL,
};

/// Manages running instance processes.
pub struct ProcessManager {
    processes: RwLock<HashMap<String, InstanceProcess>>,
    http_client: Client,
    runtime_events: broadcast::Sender<RuntimeEvent>,
}

fn read_lock_recover<'a, T>(lock: &'a RwLock<T>, name: &str) -> RwLockReadGuard<'a, T> {
    match lock.read() {
        Ok(guard) => guard,
        Err(e) => {
            log::warn!("{} read lock poisoned, recovering inner state", name);
            e.into_inner()
        }
    }
}

fn write_lock_recover<'a, T>(lock: &'a RwLock<T>, name: &str) -> RwLockWriteGuard<'a, T> {
    match lock.write() {
        Ok(guard) => guard,
        Err(e) => {
            log::warn!("{} write lock poisoned, recovering inner state", name);
            e.into_inner()
        }
    }
}

impl ProcessManager {
    #[allow(clippy::expect_used)]
    pub fn new() -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .expect("Failed to create HTTP client");

        let (runtime_events, _) = broadcast::channel(128);

        Self {
            processes: RwLock::new(HashMap::new()),
            http_client,
            runtime_events,
        }
    }

    pub fn subscribe_runtime_events(&self) -> broadcast::Receiver<RuntimeEvent> {
        self.runtime_events.subscribe()
    }

    fn emit_runtime_event(&self, instance_id: &str, reason: RuntimeEventReason) {
        let _ = self.runtime_events.send(RuntimeEvent {
            instance_id: instance_id.to_string(),
            reason,
        });
    }

    /// Handle a health check failure for a dashboard-enabled instance.
    /// Returns `true` if the instance is still in grace period (considered alive).
    fn handle_health_failure(&self, id: &str, now: Instant) -> bool {
        let mut procs = write_lock_recover(&self.processes, "ProcessManager.processes");
        let Some(info) = procs.get_mut(id) else {
            return false;
        };

        if info.health_failure_since.is_none() {
            info.health_failure_since = Some(now);
        }
        info.failure_count += 1;

        let failure_count = info.failure_count;
        let failure_start = info.health_failure_since.unwrap_or(now);
        let failure_duration = now.duration_since(failure_start);
        let grace_exceeded = failure_duration >= HEALTH_CHECK_GRACE_PERIOD;

        if !grace_exceeded {
            let backoff = info.calculate_backoff();
            info.next_check_at = Some(now + backoff);
        }

        drop(procs);

        if failure_count >= 3 {
            log::warn!(
                "Instance {} health check failed {} times, monitoring...",
                id,
                failure_count
            );
        }

        if grace_exceeded {
            log::error!(
                "Instance {} disconnected after {:?} ({} failures)",
                id,
                failure_duration,
                failure_count
            );
            false
        } else {
            true
        }
    }

    /// Remove stale instances and emit disconnect events.
    fn cleanup_stale_instances(&self, stale_instances: &[String]) {
        if stale_instances.is_empty() {
            return;
        }
        let mut disconnected_ids = Vec::new();
        let mut procs = write_lock_recover(&self.processes, "ProcessManager.processes");
        for id in stale_instances {
            if procs.remove(id).is_some() {
                log::info!("Removed stale process tracking entry for instance {}", id);
                disconnected_ids.push(id.clone());
            }
        }
        drop(procs);

        for id in disconnected_ids {
            self.emit_runtime_event(&id, RuntimeEventReason::HealthDisconnected);
        }
    }

    pub fn start_runtime_monitor(self: Arc<Self>) {
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(MONITOR_INTERVAL);
            loop {
                interval.tick().await;
                let _ = self.get_all_statuses().await;
            }
        });
    }

    /// Check if an instance is running (simple check for specific instance).
    pub async fn is_running(&self, instance_id: &str) -> bool {
        let info = {
            let procs = read_lock_recover(&self.processes, "ProcessManager.processes");
            procs.get(instance_id).cloned()
        };

        if let Some(info) = info {
            if info.dashboard_enabled && check_health(&self.http_client, info.port).await {
                return true;
            }

            is_expected_process_alive(info.pid, &info.executable_path)
        } else {
            false
        }
    }

    /// Set the process info for an instance.
    pub fn set_process(
        &self,
        instance_id: &str,
        pid: u32,
        executable_path: PathBuf,
        port: u16,
        dashboard_enabled: bool,
    ) {
        let mut procs = write_lock_recover(&self.processes, "ProcessManager.processes");
        procs.insert(
            instance_id.to_string(),
            InstanceProcess::new(pid, executable_path, port, dashboard_enabled),
        );
        drop(procs);
        self.emit_runtime_event(instance_id, RuntimeEventReason::ProcessTracked);
    }

    /// Get the port for an instance.
    pub fn get_port(&self, instance_id: &str) -> Option<u16> {
        let procs = read_lock_recover(&self.processes, "ProcessManager.processes");
        procs.get(instance_id).map(|info| info.port)
    }

    /// Remove an instance from tracking and return its process info.
    pub fn remove(&self, instance_id: &str) -> Option<InstanceProcess> {
        let mut procs = write_lock_recover(&self.processes, "ProcessManager.processes");
        let removed = procs.remove(instance_id);
        drop(procs);
        if removed.is_some() {
            self.emit_runtime_event(instance_id, RuntimeEventReason::ProcessRemoved);
        }
        removed
    }

    /// Mark that the child PID has exited, without removing the tracking entry.
    /// The runtime monitor will handle cleanup via health checks / `is_process_alive`.
    pub fn mark_pid_exited(&self, instance_id: &str, expected_pid: u32) {
        let mut procs = write_lock_recover(&self.processes, "ProcessManager.processes");
        if let Some(info) = procs.get_mut(instance_id) {
            if info.pid == expected_pid {
                info.pid_exited = true;
                log::info!(
                    "Instance {} PID {} marked as exited",
                    instance_id,
                    expected_pid
                );
            }
        }
    }

    /// Get running status for all tracked instances.
    ///
    /// Uses health endpoint check with exponential backoff:
    /// 1. Check /api/stat/start-time endpoint
    /// 2. If succeeds: update PID if needed, clear failure state
    /// 3. If fails: exponential backoff retry, grace period ~2min before marking as disconnected
    pub async fn get_all_statuses(&self) -> HashMap<String, bool> {
        let now = Instant::now();

        // Get instances to check
        let instances: Vec<(String, u16, u32, PathBuf, bool, Option<Instant>, bool)> = {
            let procs = read_lock_recover(&self.processes, "ProcessManager.processes");
            procs
                .iter()
                .map(|(id, info)| {
                    (
                        id.clone(),
                        info.port,
                        info.pid,
                        info.executable_path.clone(),
                        info.dashboard_enabled,
                        info.next_check_at,
                        info.pid_exited,
                    )
                })
                .collect()
        };

        let mut results = HashMap::new();
        let mut stale_instances = Vec::new();

        for (id, port, pid, executable_path, dashboard_enabled, next_check_at, pid_exited) in
            instances
        {
            if !dashboard_enabled {
                let alive = !pid_exited && is_expected_process_alive(pid, &executable_path);

                if alive {
                    let mut procs = write_lock_recover(&self.processes, "ProcessManager.processes");
                    if let Some(info) = procs.get_mut(&id) {
                        info.clear_health_failure_state();
                    }
                    drop(procs);
                    results.insert(id, true);
                } else {
                    stale_instances.push(id.clone());
                    results.insert(id, false);
                }

                continue;
            }

            // Check if we should skip this check (exponential backoff)
            if let Some(next_at) = next_check_at {
                if now < next_at {
                    // Not yet time to check, assume still running (in grace period)
                    results.insert(id, true);
                    continue;
                }
            }

            // Perform health check
            let is_healthy = check_health(&self.http_client, port).await;

            if is_healthy {
                // Health check passed — service is alive (may have self-restarted with a new PID)
                let mut procs = write_lock_recover(&self.processes, "ProcessManager.processes");
                if let Some(info) = procs.get_mut(&id) {
                    #[cfg(target_os = "windows")]
                    if let Some(new_pid) = get_pid_on_port(port) {
                        if new_pid != info.pid {
                            if is_expected_process_alive(new_pid, &info.executable_path) {
                                log::info!(
                                    "Instance {} PID updated: {} -> {} (port {})",
                                    id,
                                    info.pid,
                                    new_pid,
                                    port
                                );
                                info.pid = new_pid;
                            } else {
                                log::warn!(
                                    "Instance {} rejected PID update {} -> {}: executable path mismatch",
                                    id,
                                    info.pid,
                                    new_pid
                                );
                            }
                        }
                    }
                    if info.failure_count >= 3 {
                        log::info!(
                            "Instance {} health restored after {} failures",
                            id,
                            info.failure_count
                        );
                    }
                    info.pid_exited = false;
                    info.clear_health_failure_state();
                }
                drop(procs);
                results.insert(id, true);
            } else {
                // Health check failed — walk grace period regardless of PID state
                let alive = self.handle_health_failure(&id, now);
                if !alive {
                    stale_instances.push(id.clone());
                }
                results.insert(id, alive);
            }
        }

        self.cleanup_stale_instances(&stale_instances);

        results
    }

    /// Get a runtime snapshot for all tracked instances.
    pub async fn get_runtime_snapshot(&self) -> HashMap<String, InstanceRuntimeSnapshot> {
        let running = self.get_all_statuses().await;
        let procs = read_lock_recover(&self.processes, "ProcessManager.processes");

        procs
            .iter()
            .map(|(id, info)| {
                (
                    id.clone(),
                    InstanceRuntimeSnapshot {
                        running: running.get(id).copied().unwrap_or(false),
                        port: info.port,
                        dashboard_enabled: info.dashboard_enabled,
                    },
                )
            })
            .collect()
    }

    /// Wait for an instance to become healthy (startup complete).
    /// Polls the health endpoint with exponential backoff until success or timeout.
    pub async fn wait_for_startup(
        &self,
        pid: u32,
        port: u16,
        executable_path: &std::path::Path,
        timeout_secs: u64,
    ) -> std::result::Result<(), String> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(timeout_secs);
        let mut interval = std::time::Duration::from_millis(500);
        let max_interval = std::time::Duration::from_secs(2);

        loop {
            if !is_expected_process_alive(pid, executable_path) {
                return Err("Instance process exited".to_string());
            }
            if check_health(&self.http_client, port).await {
                return Ok(());
            }

            if start.elapsed() >= timeout {
                return Err(format!("Instance startup timed out ({}s)", timeout_secs));
            }

            tokio::time::sleep(interval).await;
            interval = (interval * 2).min(max_interval);
        }
    }

    /// Get the IDs of all currently tracked instances.
    ///
    /// This returns entries in the process manager map only.
    /// It does not perform runtime status checks.
    pub fn get_tracked_ids(&self) -> Vec<String> {
        let procs = read_lock_recover(&self.processes, "ProcessManager.processes");
        procs.keys().cloned().collect()
    }

    /// Stop all running instances with graceful shutdown.
    pub fn stop_all(&self) {
        let mut procs = write_lock_recover(&self.processes, "ProcessManager.processes");
        let entries: Vec<(String, InstanceProcess)> = procs.drain().collect();
        drop(procs);

        for (id, info) in &entries {
            log::info!(
                "Stopping instance {} (pid: {}, port: {})",
                id,
                info.pid,
                info.port
            );
        }

        let pids: Vec<u32> = entries
            .iter()
            .filter_map(|(_, info)| {
                if can_signal_expected_process(info.pid, &info.executable_path) {
                    Some(info.pid)
                } else {
                    log::warn!(
                        "Skip stopping PID {}: executable path mismatch (possible PID reuse)",
                        info.pid
                    );
                    None
                }
            })
            .collect();
        graceful_shutdown(&pids);
    }
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}
