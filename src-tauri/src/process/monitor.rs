//! Runtime monitoring: liveness probes, health checks, and state reconciliation.
//!
//! All evaluation functions are standalone — they take snapshot data and return results.
//! The monitor task (spawned by ProcessManager) calls `poll_instances` on a timer.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use reqwest::Client;
use tokio::sync::broadcast;

use super::manager::{derive_health_state, ProcessState, Slot};
#[cfg(target_os = "windows")]
use super::control::is_process_alive;
use super::control::is_expected_process_alive;
use super::health::check_health;
#[cfg(target_os = "windows")]
use super::win_api::get_pid_on_port;
#[cfg(target_os = "windows")]
use super::ALIVE_EXIT_THRESHOLD;
use super::{InstanceState, RuntimeEvent, UNHEALTHY_THRESHOLD};
use crate::utils::sync::lock_mutex_recover;

/// Snapshot of a single live instance for evaluation.
struct LiveSnapshot {
    id: String,
    port: u16,
    pid: u32,
    executable_path: PathBuf,
    dashboard_enabled: bool,
    next_health_check_at: Option<Instant>,
    health_failure_count: u32,
    #[cfg(target_os = "windows")]
    alive_failure_count: u32,
    #[cfg(target_os = "windows")]
    next_alive_check_at: Option<Instant>,
}

/// Outcome of evaluating a single instance's liveness and health.
enum EvalOutcome {
    /// Process is dead — slot should be removed.
    Dead,
    /// Process is alive — update its fields.
    Alive(AliveUpdate),
}

/// Fields to write back for an alive instance (one per instance, no duplicates).
struct AliveUpdate {
    computed_state: InstanceState,
    health_failure_count: u32,
    next_health_check_at: Option<Instant>,
    #[cfg(target_os = "windows")]
    alive_failure_count: u32,
    #[cfg(target_os = "windows")]
    next_alive_check_at: Option<Instant>,
    #[cfg(target_os = "windows")]
    new_pid: Option<u32>,
}

impl AliveUpdate {
    /// Preserve the snapshot's health and liveness state as-is.
    #[cfg(target_os = "windows")]
    fn from_entry_running(entry: &LiveSnapshot) -> Self {
        Self {
            computed_state: InstanceState::Running,
            health_failure_count: entry.health_failure_count,
            next_health_check_at: entry.next_health_check_at,
            alive_failure_count: entry.alive_failure_count,
            next_alive_check_at: entry.next_alive_check_at,
            new_pid: None,
        }
    }

    /// Preserve health state but reset alive tracking (liveness confirmed).
    #[cfg(target_os = "windows")]
    fn from_entry_running_reset_alive(entry: &LiveSnapshot, new_pid: Option<u32>) -> Self {
        Self {
            computed_state: InstanceState::Running,
            health_failure_count: entry.health_failure_count,
            next_health_check_at: entry.next_health_check_at,
            alive_failure_count: 0,
            next_alive_check_at: None,
            new_pid,
        }
    }

    /// Preserve health state (non-Windows: no alive tracking fields).
    #[cfg(not(target_os = "windows"))]
    fn from_entry_running(entry: &LiveSnapshot) -> Self {
        Self {
            computed_state: InstanceState::Running,
            health_failure_count: entry.health_failure_count,
            next_health_check_at: entry.next_health_check_at,
        }
    }
}

#[cfg(target_os = "windows")]
enum LivenessProbeResult {
    Alive,
    AliveWithNewPid(u32),
    Dead,
}

/// Entry point called by the monitor task on each tick.
///
/// Snap-then-apply: locks briefly to snapshot, evaluates concurrently with no
/// lock held, then locks again to apply results.
pub(super) async fn poll_instances(
    state: &Mutex<ProcessState>,
    http_client: &Client,
    runtime_events: &broadcast::Sender<RuntimeEvent>,
) {
    // Phase 1: lock → snapshot Live slots → unlock
    let entries: Vec<LiveSnapshot> = {
        let state = lock_mutex_recover(state, "ProcessState");
        if state.shutting_down {
            return;
        }
        state
            .slots
            .iter()
            .filter_map(|(id, slot)| {
                if let Slot::Live(p) = slot {
                    Some(LiveSnapshot {
                        id: id.clone(),
                        port: p.port,
                        pid: p.pid,
                        executable_path: p.executable_path.clone(),
                        dashboard_enabled: p.dashboard_enabled,
                        next_health_check_at: p.next_health_check_at,
                        health_failure_count: p.health_failure_count,
                        #[cfg(target_os = "windows")]
                        alive_failure_count: p.alive_failure_count,
                        #[cfg(target_os = "windows")]
                        next_alive_check_at: p.next_alive_check_at,
                    })
                } else {
                    None
                }
            })
            .collect()
    };

    if entries.is_empty() {
        return;
    }

    // Phase 2: evaluate (concurrent health checks)
    let outcomes = evaluate_instances(&entries, http_client).await;

    // Phase 3: lock → apply outcomes → unlock → collect events
    let events = {
        let mut state = lock_mutex_recover(state, "ProcessState");
        apply_outcomes(&mut state, &outcomes)
    };

    // Phase 4: emit events (outside lock)
    for (id, new_state) in events {
        let _ = runtime_events.send(RuntimeEvent {
            instance_id: id,
            state: new_state,
        });
    }
}

/// Evaluate all instances. Liveness checks are synchronous; health checks run
/// concurrently via `join_all`.
async fn evaluate_instances(
    entries: &[LiveSnapshot],
    http_client: &Client,
) -> Vec<(String, EvalOutcome)> {
    let now = Instant::now();
    let mut outcomes: Vec<(String, EvalOutcome)> = Vec::new();
    let mut needs_health_check: Vec<(&LiveSnapshot, AliveUpdate)> = Vec::new();

    for entry in entries {
        let Some(update) = evaluate_liveness(entry, now) else {
            outcomes.push((entry.id.clone(), EvalOutcome::Dead));
            continue;
        };

        if !entry.dashboard_enabled {
            // Alive, no dashboard → use liveness defaults directly.
            outcomes.push((entry.id.clone(), EvalOutcome::Alive(update)));
            continue;
        }

        // Dashboard enabled: health check will overwrite health fields in update.
        needs_health_check.push((entry, update));
    }

    // Concurrent health checks
    let health_futures: Vec<_> = needs_health_check
        .into_iter()
        .map(|(entry, update)| evaluate_health_for_update(entry, update, now, http_client))
        .collect();
    let health_results = futures_util::future::join_all(health_futures).await;

    outcomes.extend(health_results);
    outcomes
}

/// Evaluate health and overwrite health fields in the liveness update.
async fn evaluate_health_for_update(
    entry: &LiveSnapshot,
    mut update: AliveUpdate,
    now: Instant,
    http_client: &Client,
) -> (String, EvalOutcome) {
    let (computed_state, health_failure_count, next_health_check_at) =
        evaluate_health(entry, now, http_client).await;

    update.computed_state = computed_state;
    update.health_failure_count = health_failure_count;
    update.next_health_check_at = next_health_check_at;

    (entry.id.clone(), EvalOutcome::Alive(update))
}

/// Apply monitor outcomes to the live process state. Returns events to emit.
fn apply_outcomes(
    state: &mut ProcessState,
    outcomes: &[(String, EvalOutcome)],
) -> Vec<(String, InstanceState)> {
    let mut events = Vec::new();

    for (id, outcome) in outcomes {
        match outcome {
            EvalOutcome::Dead => {
                // Only remove slots still in Live state — another lifecycle
                // method may have transitioned the slot in the meantime.
                if matches!(state.slots.get(id), Some(Slot::Live(_))) {
                    state.slots.remove(id);
                    log::info!("Removed dead process tracking entry for instance {}", id);
                    events.push((id.clone(), InstanceState::Stopped));
                }
            }
            EvalOutcome::Alive(update) => {
                if let Some(Slot::Live(p)) = state.slots.get_mut(id) {
                    let old_state = derive_health_state(p);

                    p.health_failure_count = update.health_failure_count;
                    p.next_health_check_at = update.next_health_check_at;
                    #[cfg(target_os = "windows")]
                    {
                        p.alive_failure_count = update.alive_failure_count;
                        p.next_alive_check_at = update.next_alive_check_at;
                        if let Some(new_pid) = update.new_pid {
                            log::info!(
                                "Instance {} PID updated: {} -> {} (port {})",
                                id,
                                p.pid,
                                new_pid,
                                p.port
                            );
                            p.pid = new_pid;
                        }
                    }

                    let new_state = update.computed_state;
                    if old_state != new_state {
                        match new_state {
                            InstanceState::Unhealthy => {
                                events.push((id.clone(), InstanceState::Unhealthy));
                            }
                            InstanceState::Running if old_state == InstanceState::Unhealthy => {
                                events.push((id.clone(), InstanceState::Running));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    events
}

// -- Health evaluation --------------------------------------------------------

/// Evaluate a single instance's health (async, involves HTTP).
///
/// Returns `(computed_state, health_failure_count, next_health_check_at)`.
async fn evaluate_health(
    entry: &LiveSnapshot,
    now: Instant,
    http_client: &Client,
) -> (InstanceState, u32, Option<Instant>) {
    // Backoff: not yet time to check — use previous state.
    if let Some(next_at) = entry.next_health_check_at {
        if now < next_at {
            let state = if entry.health_failure_count >= UNHEALTHY_THRESHOLD {
                InstanceState::Unhealthy
            } else {
                InstanceState::Running
            };
            return (state, entry.health_failure_count, entry.next_health_check_at);
        }
    }

    let is_healthy = check_health(http_client, entry.port).await;

    if is_healthy {
        let was_unhealthy = entry.health_failure_count >= UNHEALTHY_THRESHOLD;
        if was_unhealthy {
            log::info!(
                "Instance {} health restored after {} failures",
                entry.id,
                entry.health_failure_count
            );
        }
        (InstanceState::Running, 0, None)
    } else {
        let new_count = entry.health_failure_count + 1;
        let backoff = calculate_health_backoff(new_count);
        let next_at = Some(now + backoff);

        let state = if new_count >= UNHEALTHY_THRESHOLD {
            if entry.health_failure_count < UNHEALTHY_THRESHOLD {
                log::warn!(
                    "Instance {} marked unhealthy after {} consecutive health check failures",
                    entry.id,
                    new_count
                );
            }
            InstanceState::Unhealthy
        } else {
            InstanceState::Running
        };

        (state, new_count, next_at)
    }
}

/// Capped exponential backoff: `2^count` seconds, clamped to [`MAX_BACKOFF`](super::MAX_BACKOFF).
fn calculate_health_backoff(failure_count: u32) -> std::time::Duration {
    let secs = 1u64 << failure_count.min(5); // 1, 2, 4, 8, 16, 32
    std::time::Duration::from_secs(secs).min(super::MAX_BACKOFF)
}

// -- Liveness evaluation ------------------------------------------------------

#[cfg(target_os = "windows")]
fn evaluate_liveness(entry: &LiveSnapshot, now: Instant) -> Option<AliveUpdate> {
    // Backoff check
    if entry.dashboard_enabled {
        if let Some(next_at) = entry.next_alive_check_at {
            if now < next_at {
                return Some(AliveUpdate::from_entry_running(entry));
            }
        }
    }

    match probe_liveness(entry) {
        LivenessProbeResult::Alive => {
            Some(AliveUpdate::from_entry_running_reset_alive(entry, None))
        }
        LivenessProbeResult::AliveWithNewPid(new_pid) => {
            Some(AliveUpdate::from_entry_running_reset_alive(entry, Some(new_pid)))
        }
        LivenessProbeResult::Dead => {
            if !entry.dashboard_enabled {
                None
            } else {
                handle_liveness_failure(entry, now)
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn handle_liveness_failure(entry: &LiveSnapshot, now: Instant) -> Option<AliveUpdate> {
    let new_count = entry.alive_failure_count + 1;
    let backoff = calculate_health_backoff(new_count);

    if new_count >= ALIVE_EXIT_THRESHOLD {
        log::warn!(
            "Instance {} liveness probe failed {} times, treating process as exited",
            entry.id,
            new_count
        );
        None
    } else {
        log::debug!(
            "Instance {} liveness probe failed (count: {}), retry in {}s",
            entry.id,
            new_count,
            backoff.as_secs()
        );
        Some(AliveUpdate {
            alive_failure_count: new_count,
            next_alive_check_at: Some(now + backoff),
            ..AliveUpdate::from_entry_running_reset_alive(entry, None)
        })
    }
}

#[cfg(target_os = "windows")]
fn probe_liveness(entry: &LiveSnapshot) -> LivenessProbeResult {
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
fn evaluate_liveness(entry: &LiveSnapshot, _now: Instant) -> Option<AliveUpdate> {
    is_expected_process_alive(entry.pid, &entry.executable_path)
        .then(|| AliveUpdate::from_entry_running(entry))
}
