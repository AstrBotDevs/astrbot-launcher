//! Runtime monitoring: liveness probes, health checks, and state reconciliation.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use crate::utils::sync::{read_lock_recover, write_lock_recover};

#[cfg(target_os = "windows")]
use super::control::is_process_alive;
use super::control::is_expected_process_alive;
use super::health::check_health;
use super::manager::ProcessManager;
#[cfg(target_os = "windows")]
use super::win_api::get_pid_on_port;
#[cfg(target_os = "windows")]
use super::ALIVE_EXIT_THRESHOLD;
use super::{InstanceState, MONITOR_INTERVAL, UNHEALTHY_THRESHOLD};

/// Snapshot of an instance's state for the status check loop.
struct InstanceCheckEntry {
    id: String,
    port: u16,
    pid: u32,
    executable_path: PathBuf,
    dashboard_enabled: bool,
    next_health_check_at: Option<Instant>,
    instance_state: InstanceState,
    #[cfg(target_os = "windows")]
    alive_failure_count: u32,
    #[cfg(target_os = "windows")]
    next_alive_check_at: Option<Instant>,
}

#[cfg(target_os = "windows")]
enum LivenessProbeResult {
    Alive,
    AliveWithNewPid(u32),
    Dead,
}

impl ProcessManager {
    /// Start the background monitor that periodically polls all instances.
    pub fn start_runtime_monitor(self: Arc<Self>) {
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(MONITOR_INTERVAL);
            loop {
                interval.tick().await;
                self.poll_instances().await;
            }
        });
    }

    /// Evaluate liveness and health for every tracked instance, updating
    /// internal state and emitting events as needed.
    ///
    /// - All instances: evaluate liveness first; dead → `Stopped` (remove).
    ///   - On Windows, dashboard-enabled instances use liveness backoff and port→PID fallback;
    ///     dashboard-disabled instances are stopped immediately when liveness validation fails.
    /// - dashboard_enabled + alive: health check with exponential backoff.
    ///   - healthy → `Running`
    ///   - failures < UNHEALTHY_THRESHOLD → `Running` (tolerate)
    ///   - failures >= UNHEALTHY_THRESHOLD → `Unhealthy`, emit event
    /// - dashboard_disabled + alive → `Running`
    pub(super) async fn poll_instances(&self) {
        let now = Instant::now();

        // Snapshot all instances under a short-lived read lock.
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
                    next_health_check_at: info.next_health_check_at,
                    instance_state: info.state,
                    #[cfg(target_os = "windows")]
                    alive_failure_count: info.alive_failure_count,
                    #[cfg(target_os = "windows")]
                    next_alive_check_at: info.next_alive_check_at,
                })
                .collect()
        };

        let mut results = HashMap::new();
        let mut dead_instances = Vec::new();

        for entry in instances {
            // Skip instances in transitional states managed by lifecycle code.
            if matches!(
                entry.instance_state,
                InstanceState::Starting | InstanceState::Stopping
            ) {
                results.insert(entry.id, entry.instance_state);
                continue;
            }

            // First: evaluate process liveness.
            if !self.evaluate_liveness(&entry, now) {
                dead_instances.push(entry.id.clone());
                results.insert(entry.id, InstanceState::Stopped);
                continue;
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

            // Dashboard enabled: perform health check with backoff.
            let state = self.evaluate_health(&entry, now).await;
            results.insert(entry.id, state);
        }

        // Remove dead instances.
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

        // Sync computed states back, but never overwrite transitional states
        // that external code (begin_stop / set_state) may have set while we
        // were running async health checks.
        {
            let mut procs = write_lock_recover(&self.processes, "ProcessManager.processes");
            for (id, state) in &results {
                if let Some(info) = procs.get_mut(id) {
                    if matches!(
                        info.state,
                        InstanceState::Starting | InstanceState::Stopping
                    ) {
                        continue;
                    }
                    info.state = *state;
                }
            }
        }
    }

    // ── Health evaluation ────────────────────────────────────────────────

    /// Evaluate the health of a single dashboard-enabled instance.
    ///
    /// Returns the computed `InstanceState` after performing (or skipping) a
    /// health check with exponential backoff.
    async fn evaluate_health(&self, entry: &InstanceCheckEntry, now: Instant) -> InstanceState {
        // Backoff: not yet time to check — use previous state.
        if let Some(next_at) = entry.next_health_check_at {
            if now < next_at {
                let procs = read_lock_recover(&self.processes, "ProcessManager.processes");
                return if procs
                    .get(&entry.id)
                    .is_some_and(|info| info.health_failure_count >= UNHEALTHY_THRESHOLD)
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
            let was_unhealthy = info.health_failure_count >= UNHEALTHY_THRESHOLD;
            if was_unhealthy {
                log::info!(
                    "Instance {} health restored after {} failures",
                    entry.id,
                    info.health_failure_count
                );
            }
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
            let was_below_threshold = info.health_failure_count < UNHEALTHY_THRESHOLD;
            info.health_failure_count += 1;
            let backoff = info.calculate_health_backoff();
            info.next_health_check_at = Some(now + backoff);

            if info.health_failure_count >= UNHEALTHY_THRESHOLD {
                if was_below_threshold {
                    log::warn!(
                        "Instance {} marked unhealthy after {} consecutive health check failures",
                        entry.id,
                        info.health_failure_count
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

    // ── Liveness evaluation ──────────────────────────────────────────────

    #[cfg(target_os = "windows")]
    fn evaluate_liveness(&self, entry: &InstanceCheckEntry, now: Instant) -> bool {
        if self.is_liveness_check_in_backoff(entry, now) {
            return true;
        }

        match self.probe_liveness(entry) {
            LivenessProbeResult::Alive => {
                self.clear_alive_failure_state_if_needed(entry);
                true
            }
            LivenessProbeResult::AliveWithNewPid(new_pid) => {
                self.update_pid_after_probe(entry, new_pid)
            }
            LivenessProbeResult::Dead => {
                // `dashboard_enabled` currently gates Windows liveness retry/backoff behavior.
                if !entry.dashboard_enabled {
                    return false;
                }
                self.handle_liveness_failure(entry, now)
            }
        }
    }

    #[cfg(target_os = "windows")]
    fn is_liveness_check_in_backoff(&self, entry: &InstanceCheckEntry, now: Instant) -> bool {
        entry.dashboard_enabled
            && entry
                .next_alive_check_at
                .is_some_and(|next_at| now < next_at)
    }

    #[cfg(target_os = "windows")]
    fn clear_alive_failure_state_if_needed(&self, entry: &InstanceCheckEntry) {
        if entry.alive_failure_count == 0 && entry.next_alive_check_at.is_none() {
            return;
        }

        let mut procs = write_lock_recover(&self.processes, "ProcessManager.processes");
        if let Some(info) = procs.get_mut(&entry.id) {
            info.clear_alive_failure_state();
        }
    }

    #[cfg(target_os = "windows")]
    fn update_pid_after_probe(&self, entry: &InstanceCheckEntry, new_pid: u32) -> bool {
        let mut procs = write_lock_recover(&self.processes, "ProcessManager.processes");
        if let Some(info) = procs.get_mut(&entry.id) {
            log::info!(
                "Instance {} PID updated: {} -> {} (port {})",
                entry.id,
                info.pid,
                new_pid,
                entry.port
            );
            info.pid = new_pid;
            info.clear_alive_failure_state();
            return true;
        }
        false
    }

    #[cfg(target_os = "windows")]
    fn handle_liveness_failure(&self, entry: &InstanceCheckEntry, now: Instant) -> bool {
        let (should_stop, current_failures, backoff_secs) = {
            let mut procs = write_lock_recover(&self.processes, "ProcessManager.processes");
            let Some(info) = procs.get_mut(&entry.id) else {
                return false;
            };

            info.alive_failure_count += 1;
            let backoff = info.calculate_alive_backoff();
            info.next_alive_check_at = Some(now + backoff);
            (
                info.alive_failure_count >= ALIVE_EXIT_THRESHOLD,
                info.alive_failure_count,
                backoff.as_secs(),
            )
        };

        if should_stop {
            log::warn!(
                "Instance {} liveness probe failed {} times, treating process as exited",
                entry.id,
                current_failures
            );
            false
        } else {
            log::debug!(
                "Instance {} liveness probe failed (count: {}), retry in {}s",
                entry.id,
                current_failures,
                backoff_secs
            );
            true
        }
    }

    #[cfg(target_os = "windows")]
    fn probe_liveness(&self, entry: &InstanceCheckEntry) -> LivenessProbeResult {
        if is_expected_process_alive(entry.pid, &entry.executable_path) {
            return LivenessProbeResult::Alive;
        }

        if let Some(new_pid) = get_pid_on_port(entry.port) {
            if new_pid == entry.pid {
                if entry.alive_failure_count == 0 {
                    log::warn!(
                        "Instance {} liveness probe failed for PID {}, but port {} still resolves to the same PID",
                        entry.id,
                        entry.pid,
                        entry.port
                    );
                } else {
                    log::debug!(
                        "Instance {} still resolves port {} to PID {} while liveness probe remains failed",
                        entry.id,
                        entry.port,
                        entry.pid
                    );
                }
                return LivenessProbeResult::Dead;
            }

            if is_expected_process_alive(new_pid, &entry.executable_path) {
                return LivenessProbeResult::AliveWithNewPid(new_pid);
            }

            if is_process_alive(new_pid) {
                log::warn!(
                    "Instance {} rejected PID update {} -> {}: executable path mismatch",
                    entry.id,
                    entry.pid,
                    new_pid
                );
            } else {
                log::debug!(
                    "Instance {} observed transient PID {} on port {}, but process was not alive during validation",
                    entry.id,
                    new_pid,
                    entry.port
                );
            }
        }

        LivenessProbeResult::Dead
    }

    #[cfg(not(target_os = "windows"))]
    fn evaluate_liveness(&self, entry: &InstanceCheckEntry, _now: Instant) -> bool {
        is_expected_process_alive(entry.pid, &entry.executable_path)
    }
}
