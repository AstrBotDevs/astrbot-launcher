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
use super::control::is_expected_process_alive;
use super::health::check_health;
use super::{InstanceState, RuntimeEvent, UNHEALTHY_THRESHOLD};
use crate::utils::sync::lock_mutex_recover;

#[cfg(target_os = "windows")]
use super::control::is_process_alive;
#[cfg(target_os = "windows")]
use super::win_api::get_pid_on_port;
#[cfg(target_os = "windows")]
use super::ALIVE_EXIT_THRESHOLD;

/// Snapshot of a single live instance for evaluation.
struct LiveSnapshot {
    id: String,
    port: u16,
    pid: u32,
    executable_path: PathBuf,
    dashboard_enabled: bool,
    next_health_check_at: Option<Instant>,
    health_failure_count: u32,
    alive_failure_count: u32,
    next_alive_check_at: Option<Instant>,
}

/// Fields to write back for an alive instance (one per instance, no duplicates).
///
/// Evaluation results use `Option<AliveUpdate>`: `None` means the process is
/// dead (slot should be removed), `Some` means it is alive (update its fields).
///
/// Carries only counters, timestamps, and optional PID updates — the derived
/// `InstanceState` is computed from `derive_health_state` when applying outcomes,
/// keeping a single source of truth.
struct AliveUpdate {
    health_failure_count: u32,
    next_health_check_at: Option<Instant>,
    /// Always 0 / `None` on non-Windows (no retry mechanism).
    alive_failure_count: u32,
    next_alive_check_at: Option<Instant>,
    new_pid: Option<u32>,
}

/// Platform-abstracted result of a single liveness probe.
enum LivenessOutcome {
    Dead,
    AliveSamePid,
    AliveNewPid(u32),
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
                        alive_failure_count: p.alive_failure_count,
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
) -> Vec<(String, Option<AliveUpdate>)> {
    let now = Instant::now();
    let mut outcomes: Vec<(String, Option<AliveUpdate>)> = Vec::new();
    let mut needs_health_check: Vec<(&LiveSnapshot, AliveUpdate)> = Vec::new();

    for entry in entries {
        let Some(update) = evaluate_liveness(entry, now) else {
            outcomes.push((entry.id.clone(), None));
            continue;
        };

        if !entry.dashboard_enabled {
            // Alive, no dashboard → use liveness defaults directly.
            outcomes.push((entry.id.clone(), Some(update)));
            continue;
        }

        // Dashboard enabled: health check will overwrite health fields in update.
        needs_health_check.push((entry, update));
    }

    // Concurrent health checks (inlined — no separate helper function)
    let health_futures: Vec<_> = needs_health_check
        .into_iter()
        .map(|(entry, mut update)| {
            let http_client = http_client.clone();
            async move {
                let (health_failure_count, next_health_check_at) =
                    evaluate_health(entry, now, &http_client).await;
                update.health_failure_count = health_failure_count;
                update.next_health_check_at = next_health_check_at;
                (entry.id.clone(), Some(update))
            }
        })
        .collect();
    let health_results = futures_util::future::join_all(health_futures).await;

    outcomes.extend(health_results);
    outcomes
}

/// Apply monitor outcomes to the live process state. Returns events to emit.
fn apply_outcomes(
    state: &mut ProcessState,
    outcomes: &[(String, Option<AliveUpdate>)],
) -> Vec<(String, InstanceState)> {
    let mut events = Vec::new();

    for (id, update) in outcomes {
        match update {
            None => {
                // Only remove slots still in Live state — another lifecycle
                // method may have transitioned the slot in the meantime.
                if matches!(state.slots.get(id), Some(Slot::Live(_))) {
                    state.slots.remove(id);
                    log::info!("Removed dead process tracking entry for instance {}", id);
                    events.push((id.clone(), InstanceState::Stopped));
                }
            }
            Some(update) => {
                if let Some(Slot::Live(p)) = state.slots.get_mut(id) {
                    let old_state = derive_health_state(p);

                    p.health_failure_count = update.health_failure_count;
                    p.next_health_check_at = update.next_health_check_at;
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

                    // Single source of truth: derive state from updated counters.
                    let new_state = derive_health_state(p);
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
/// Returns `(health_failure_count, next_health_check_at)`. The caller derives
/// the `InstanceState` from the updated counters via `derive_health_state`.
async fn evaluate_health(
    entry: &LiveSnapshot,
    now: Instant,
    http_client: &Client,
) -> (u32, Option<Instant>) {
    // Backoff: not yet time to check — preserve previous counters.
    if let Some(next_at) = entry.next_health_check_at {
        if now < next_at {
            return (entry.health_failure_count, entry.next_health_check_at);
        }
    }

    let is_healthy = check_health(http_client, entry.port).await;

    if is_healthy {
        if entry.health_failure_count >= UNHEALTHY_THRESHOLD {
            log::info!(
                "Instance {} health restored after {} failures",
                entry.id,
                entry.health_failure_count
            );
        }
        (0, None)
    } else {
        let new_count = entry.health_failure_count + 1;
        let backoff = calculate_health_backoff(new_count);
        let next_at = Some(now + backoff);

        if new_count >= UNHEALTHY_THRESHOLD && entry.health_failure_count < UNHEALTHY_THRESHOLD {
            log::warn!(
                "Instance {} marked unhealthy after {} consecutive health check failures",
                entry.id,
                new_count
            );
        }

        (new_count, next_at)
    }
}

/// Capped exponential backoff: `2^count` seconds, clamped to [`MAX_BACKOFF`](super::MAX_BACKOFF).
fn calculate_health_backoff(failure_count: u32) -> std::time::Duration {
    let secs = 1u64 << failure_count.min(5); // 1, 2, 4, 8, 16, 32
    std::time::Duration::from_secs(secs).min(super::MAX_BACKOFF)
}

// -- Liveness evaluation ------------------------------------------------------

/// Evaluate liveness for a single instance. Returns `None` if the process is
/// dead (slot should be removed), `Some` if alive (with updated counters).
///
/// Platform-specific probing is delegated to [`probe_liveness`]. The backoff
/// and retry logic for Windows liveness failures is handled there as well.
fn evaluate_liveness(entry: &LiveSnapshot, now: Instant) -> Option<AliveUpdate> {
    // Backoff: not yet time to probe — preserve previous counters.
    // On non-Windows this is always a no-op (next_alive_check_at is always None).
    if entry.dashboard_enabled {
        if let Some(next_at) = entry.next_alive_check_at {
            if now < next_at {
                return Some(AliveUpdate {
                    health_failure_count: entry.health_failure_count,
                    next_health_check_at: entry.next_health_check_at,
                    alive_failure_count: entry.alive_failure_count,
                    next_alive_check_at: entry.next_alive_check_at,
                    new_pid: None,
                });
            }
        }
    }

    let (outcome, alive_failure_count, next_alive_check_at) = probe_liveness(entry, now);

    match outcome {
        LivenessOutcome::Dead => {
            if !entry.dashboard_enabled {
                None
            } else {
                // Windows retry: keep the slot alive until the threshold is reached.
                // probe_liveness returns None-equivalent via Dead when threshold exceeded,
                // but here we still have counters — check if we should remove.
                if alive_failure_count == 0 && next_alive_check_at.is_none() {
                    // Threshold exceeded in probe_liveness — treat as definitively dead.
                    None
                } else {
                    Some(AliveUpdate {
                        health_failure_count: entry.health_failure_count,
                        next_health_check_at: entry.next_health_check_at,
                        alive_failure_count,
                        next_alive_check_at,
                        new_pid: None,
                    })
                }
            }
        }
        LivenessOutcome::AliveSamePid => Some(AliveUpdate {
            health_failure_count: entry.health_failure_count,
            next_health_check_at: entry.next_health_check_at,
            alive_failure_count: 0,
            next_alive_check_at: None,
            new_pid: None,
        }),
        LivenessOutcome::AliveNewPid(new_pid) => Some(AliveUpdate {
            health_failure_count: entry.health_failure_count,
            next_health_check_at: entry.next_health_check_at,
            alive_failure_count: 0,
            next_alive_check_at: None,
            new_pid: Some(new_pid),
        }),
    }
}

// -- Platform-specific liveness probing ---------------------------------------

#[cfg(target_os = "windows")]
fn probe_liveness(
    entry: &LiveSnapshot,
    now: Instant,
) -> (LivenessOutcome, u32, Option<Instant>) {
    if is_expected_process_alive(entry.pid, &entry.executable_path) {
        return (LivenessOutcome::AliveSamePid, 0, None);
    }

    // PID check failed — try port-based PID discovery.
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
            // Fall through to failure handling below.
        } else if is_expected_process_alive(new_pid, &entry.executable_path) {
            return (LivenessOutcome::AliveNewPid(new_pid), 0, None);
        } else if is_process_alive(new_pid) {
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

    // Liveness probe failed — apply retry/threshold logic.
    let new_count = entry.alive_failure_count + 1;

    if new_count >= ALIVE_EXIT_THRESHOLD {
        log::warn!(
            "Instance {} liveness probe failed {} times, treating process as exited",
            entry.id,
            new_count
        );
        // Signal definitive death: zeroed counters tell the caller to remove the slot.
        (LivenessOutcome::Dead, 0, None)
    } else {
        let backoff = calculate_health_backoff(new_count);
        log::debug!(
            "Instance {} liveness probe failed (count: {}), retry in {}s",
            entry.id,
            new_count,
            backoff.as_secs()
        );
        (LivenessOutcome::Dead, new_count, Some(now + backoff))
    }
}

#[cfg(not(target_os = "windows"))]
fn probe_liveness(
    entry: &LiveSnapshot,
    _now: Instant,
) -> (LivenessOutcome, u32, Option<Instant>) {
    if is_expected_process_alive(entry.pid, &entry.executable_path) {
        (LivenessOutcome::AliveSamePid, 0, None)
    } else {
        (LivenessOutcome::Dead, 0, None)
    }
}
