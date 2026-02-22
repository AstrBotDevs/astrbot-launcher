//! Instance process tracking and runtime monitoring.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use reqwest::Client;
use tokio::sync::broadcast;

use crate::sync_utils::{read_lock_recover, write_lock_recover};

use super::control::{graceful_shutdown, is_expected_process_alive};
use super::health::check_health;
#[cfg(target_os = "windows")]
use super::win_api::get_pid_on_port;
#[cfg(target_os = "windows")]
use super::PROCESS_EXIT_THRESHOLD;
use super::{
    InstanceProcess, InstanceRuntimeSnapshot, InstanceState, RuntimeEvent, MONITOR_INTERVAL,
    UNHEALTHY_THRESHOLD,
};

/// Snapshot of an instance's state for the status check loop.
struct InstanceCheckEntry {
    id: String,
    port: u16,
    pid: u32,
    executable_path: PathBuf,
    dashboard_enabled: bool,
    next_check_at: Option<Instant>,
    pid_exited: bool,
    instance_state: InstanceState,
    #[cfg(target_os = "windows")]
    failure_count: u32,
}

/// Manages running instance processes.
pub struct ProcessManager {
    processes: RwLock<HashMap<String, InstanceProcess>>,
    http_client: Client,
    runtime_events: broadcast::Sender<RuntimeEvent>,
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

    fn emit_runtime_event(&self, instance_id: &str, state: InstanceState) {
        let _ = self.runtime_events.send(RuntimeEvent {
            instance_id: instance_id.to_string(),
            state,
        });
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

    /// Check if an instance is tracked (i.e. not stopped).
    pub fn is_tracked(&self, instance_id: &str) -> bool {
        let procs = read_lock_recover(&self.processes, "ProcessManager.processes");
        procs.contains_key(instance_id)
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
        self.emit_runtime_event(instance_id, InstanceState::Starting);
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
            self.emit_runtime_event(instance_id, InstanceState::Stopped);
        }
        removed
    }

    /// Transition an instance to Stopping state, returning process info for shutdown.
    /// Returns None if the instance is not tracked.
    pub fn begin_stop(&self, instance_id: &str) -> Option<(u32, PathBuf)> {
        let mut procs = write_lock_recover(&self.processes, "ProcessManager.processes");
        if let Some(info) = procs.get_mut(instance_id) {
            info.state = InstanceState::Stopping;
            let result = (info.pid, info.executable_path.clone());
            drop(procs);
            self.emit_runtime_event(instance_id, InstanceState::Stopping);
            Some(result)
        } else {
            None
        }
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

    /// Update the state of a tracked instance.
    pub fn set_state(&self, instance_id: &str, state: InstanceState) {
        let mut procs = write_lock_recover(&self.processes, "ProcessManager.processes");
        if let Some(info) = procs.get_mut(instance_id) {
            info.state = state;
        }
    }

    /// Evaluate the health of a single dashboard-enabled instance.
    ///
    /// Returns the computed `InstanceState` after performing (or skipping) a
    /// health check with exponential backoff.
    async fn evaluate_health(&self, entry: &InstanceCheckEntry, now: Instant) -> InstanceState {
        // Backoff: not yet time to check — use previous state
        if let Some(next_at) = entry.next_check_at {
            if now < next_at {
                let procs = read_lock_recover(&self.processes, "ProcessManager.processes");
                return if procs
                    .get(&entry.id)
                    .is_some_and(|info| info.failure_count >= UNHEALTHY_THRESHOLD)
                {
                    InstanceState::Unhealthy
                } else {
                    InstanceState::Running
                };
            }
        }

        let is_healthy = check_health(&self.http_client, entry.port).await;

        if is_healthy {
            self.handle_healthy_check(entry);
            InstanceState::Running
        } else {
            self.handle_failed_check(entry, now)
        }
    }

    /// Update tracking state after a successful health check.
    fn handle_healthy_check(&self, entry: &InstanceCheckEntry) {
        let mut procs = write_lock_recover(&self.processes, "ProcessManager.processes");
        if let Some(info) = procs.get_mut(&entry.id) {
            #[cfg(target_os = "windows")]
            if let Some(new_pid) = get_pid_on_port(entry.port) {
                if new_pid != info.pid {
                    if is_expected_process_alive(new_pid, &info.executable_path) {
                        log::info!(
                            "Instance {} PID updated: {} -> {} (port {})",
                            entry.id,
                            info.pid,
                            new_pid,
                            entry.port
                        );
                        info.pid = new_pid;
                    } else {
                        log::warn!(
                            "Instance {} rejected PID update {} -> {}: executable path mismatch",
                            entry.id,
                            info.pid,
                            new_pid
                        );
                    }
                }
            }
            let was_unhealthy = info.failure_count >= UNHEALTHY_THRESHOLD;
            if was_unhealthy {
                log::info!(
                    "Instance {} health restored after {} failures",
                    entry.id,
                    info.failure_count
                );
            }
            info.pid_exited = false;
            info.clear_health_failure_state();

            if was_unhealthy {
                drop(procs);
                self.emit_runtime_event(&entry.id, InstanceState::Running);
            }
        }
    }

    /// Update tracking state after a failed health check.
    fn handle_failed_check(&self, entry: &InstanceCheckEntry, now: Instant) -> InstanceState {
        let mut procs = write_lock_recover(&self.processes, "ProcessManager.processes");
        let mut emit_unhealthy_event = false;
        let state = if let Some(info) = procs.get_mut(&entry.id) {
            let was_below_threshold = info.failure_count < UNHEALTHY_THRESHOLD;
            info.failure_count += 1;
            let backoff = info.calculate_backoff();
            info.next_check_at = Some(now + backoff);

            if info.failure_count >= UNHEALTHY_THRESHOLD {
                if was_below_threshold {
                    log::warn!(
                        "Instance {} marked unhealthy after {} consecutive health check failures",
                        entry.id,
                        info.failure_count
                    );
                    emit_unhealthy_event = true;
                }
                InstanceState::Unhealthy
            } else {
                InstanceState::Running
            }
        } else {
            InstanceState::Stopped
        };
        drop(procs);

        if emit_unhealthy_event {
            self.emit_runtime_event(&entry.id, InstanceState::Unhealthy);
        }

        state
    }

    /// Get state for all tracked instances.
    ///
    /// - All instances: check `is_expected_process_alive` first; dead → `Stopped` (remove).
    ///   - On Windows with dashboard enabled: defer exit until health check failures
    ///     reach `PROCESS_EXIT_THRESHOLD` to tolerate unreliable process alive checks.
    /// - dashboard_enabled + alive: health check with exponential backoff.
    ///   - healthy → `Running` (update PID on Windows if needed)
    ///   - failures < UNHEALTHY_THRESHOLD → `Running` (tolerate)
    ///   - failures >= UNHEALTHY_THRESHOLD → `Unhealthy`, emit event
    /// - dashboard_disabled + alive → `Running`
    pub async fn get_all_statuses(&self) -> HashMap<String, InstanceState> {
        let now = Instant::now();

        // Get instances to check
        let instances: Vec<InstanceCheckEntry> = {
            let procs = read_lock_recover(&self.processes, "ProcessManager.processes");
            procs
                .iter()
                .map(|(id, info)| InstanceCheckEntry {
                    id: id.clone(),
                    port: info.port,
                    pid: info.pid,
                    executable_path: info.executable_path.clone(),
                    dashboard_enabled: info.dashboard_enabled,
                    next_check_at: info.next_check_at,
                    pid_exited: info.pid_exited,
                    instance_state: info.state,
                    #[cfg(target_os = "windows")]
                    failure_count: info.failure_count,
                })
                .collect()
        };

        let mut results = HashMap::new();
        let mut dead_instances = Vec::new();

        for entry in instances {
            // Skip health check for instances in transitional states
            if matches!(
                entry.instance_state,
                InstanceState::Starting | InstanceState::Stopping
            ) {
                results.insert(entry.id, entry.instance_state);
                continue;
            }

            // First: check if the process is alive
            let alive =
                !entry.pid_exited && is_expected_process_alive(entry.pid, &entry.executable_path);

            if !alive {
                #[cfg(target_os = "windows")]
                let defer_to_health_check =
                    entry.dashboard_enabled && entry.failure_count < PROCESS_EXIT_THRESHOLD;
                #[cfg(not(target_os = "windows"))]
                let defer_to_health_check = false;

                if !defer_to_health_check {
                    dead_instances.push(entry.id.clone());
                    results.insert(entry.id, InstanceState::Stopped);
                    continue;
                }
            }

            if !entry.dashboard_enabled {
                // Process alive, no dashboard → Running
                let mut procs = write_lock_recover(&self.processes, "ProcessManager.processes");
                if let Some(info) = procs.get_mut(&entry.id) {
                    info.clear_health_failure_state();
                }
                drop(procs);
                results.insert(entry.id, InstanceState::Running);
                continue;
            }

            // Dashboard enabled: perform health check with backoff
            let state = self.evaluate_health(&entry, now).await;
            results.insert(entry.id, state);
        }

        // Remove dead instances
        if !dead_instances.is_empty() {
            let mut procs = write_lock_recover(&self.processes, "ProcessManager.processes");
            let mut removed_instances = Vec::new();
            for id in &dead_instances {
                if procs.remove(id).is_some() {
                    log::info!("Removed dead process tracking entry for instance {}", id);
                    removed_instances.push(id.clone());
                }
            }
            drop(procs);

            for id in removed_instances {
                self.emit_runtime_event(&id, InstanceState::Stopped);
            }
        }

        // Sync computed states back to the tracked entries so that
        // `info.state` always reflects the latest runtime state.
        {
            let mut procs = write_lock_recover(&self.processes, "ProcessManager.processes");
            for (id, state) in &results {
                if let Some(info) = procs.get_mut(id) {
                    info.state = *state;
                }
            }
        }

        results
    }

    /// Get a runtime snapshot for all tracked instances.
    pub async fn get_runtime_snapshot(&self) -> HashMap<String, InstanceRuntimeSnapshot> {
        let statuses = self.get_all_statuses().await;
        let procs = read_lock_recover(&self.processes, "ProcessManager.processes");

        procs
            .iter()
            .map(|(id, info)| {
                (
                    id.clone(),
                    InstanceRuntimeSnapshot {
                        state: statuses.get(id).copied().unwrap_or(InstanceState::Stopped),
                        port: info.port,
                        dashboard_enabled: info.dashboard_enabled,
                    },
                )
            })
            .collect()
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

        let targets: Vec<(u32, &std::path::Path)> = entries
            .iter()
            .map(|(_, info)| (info.pid, info.executable_path.as_path()))
            .collect();
        graceful_shutdown(&targets);
    }
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}
