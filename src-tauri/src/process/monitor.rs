//! Runtime monitoring: liveness probes, health checks, and state reconciliation.
//!
//! All evaluation functions are standalone — they take snapshot data and return results.
//! The monitor task (spawned by ProcessManager) calls `poll_instances` on a timer.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use reqwest::Client;
use tokio::sync::broadcast;

use super::control::is_expected_process_alive;
use super::health::check_health;
use super::manager::{derive_health_state, InstanceEntry, ProcessState, Slot};
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

/// Outcome of evaluating a single instance during monitoring.
///
/// Collapses the previous `Option<AliveUpdate>` encoding: `Dead` replaces
/// `None` and `Alive` replaces `Some(AliveUpdate)`.
enum MonitorOutcome {
    /// The process is dead — its slot should be removed.
    Dead { id: String },
    /// The process is alive — update its fields.
    Alive {
        id: String,
        health_failure_count: u32,
        next_health_check_at: Option<Instant>,
        /// Always 0 / `None` on non-Windows (no retry mechanism).
        alive_failure_count: u32,
        next_alive_check_at: Option<Instant>,
        new_pid: Option<u32>,
    },
}

/// Entry point called by the monitor task on each tick.
///
/// Snap-then-apply: locks briefly to snapshot, evaluates concurrently with no
/// lock held, then locks again to apply results.
pub(super) async fn poll_instances(
    state: &Mutex<ProcessState>,
    http_client: Option<&Client>,
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
            .filter_map(|(id, entry)| {
                if let Some(Slot::Live(p)) = &entry.slot {
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
    http_client: Option<&Client>,
) -> Vec<MonitorOutcome> {
    match http_client {
        Some(client) => evaluate_instances_with_health(entries, client).await,
        None => evaluate_instances_liveness_only(entries),
    }
}

/// Liveness-only evaluation when no HTTP client is available.
fn evaluate_instances_liveness_only(entries: &[LiveSnapshot]) -> Vec<MonitorOutcome> {
    let now = Instant::now();
    let mut outcomes = Vec::new();

    for entry in entries {
        let Some(outcome) = evaluate_liveness(entry, now) else {
            outcomes.push(MonitorOutcome::Dead {
                id: entry.id.clone(),
            });
            continue;
        };

        outcomes.push(outcome);
    }

    outcomes
}

/// Full evaluation with concurrent HTTP health checks for dashboard-enabled instances.
async fn evaluate_instances_with_health(
    entries: &[LiveSnapshot],
    http_client: &Client,
) -> Vec<MonitorOutcome> {
    let now = Instant::now();
    let mut outcomes: Vec<MonitorOutcome> = Vec::new();
    let mut needs_health_check: Vec<(&LiveSnapshot, MonitorOutcome)> = Vec::new();

    for entry in entries {
        let Some(outcome) = evaluate_liveness(entry, now) else {
            outcomes.push(MonitorOutcome::Dead {
                id: entry.id.clone(),
            });
            continue;
        };

        if !entry.dashboard_enabled {
            // Alive, no dashboard → use liveness defaults directly.
            outcomes.push(outcome);
            continue;
        }

        // Dashboard enabled: health check will overwrite health fields in outcome.
        needs_health_check.push((entry, outcome));
    }

    // Concurrent health checks
    let health_futures: Vec<_> = needs_health_check
        .into_iter()
        .map(|(entry, outcome)| async move {
            let (health_failure_count, next_health_check_at) =
                evaluate_health(entry, now, http_client).await;
            match outcome {
                MonitorOutcome::Alive {
                    id,
                    alive_failure_count,
                    next_alive_check_at,
                    new_pid,
                    ..
                } => MonitorOutcome::Alive {
                    id,
                    health_failure_count,
                    next_health_check_at,
                    alive_failure_count,
                    next_alive_check_at,
                    new_pid,
                },
                dead @ MonitorOutcome::Dead { .. } => dead,
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
    outcomes: &[MonitorOutcome],
) -> Vec<(String, InstanceState)> {
    let mut events = Vec::new();

    for outcome in outcomes {
        match outcome {
            MonitorOutcome::Dead { id } => {
                // Only remove slots still in Live state — another lifecycle
                // method may have transitioned the slot in the meantime.
                if matches!(
                    state.slots.get(id),
                    Some(InstanceEntry {
                        slot: Some(Slot::Live(_)),
                        ..
                    })
                ) {
                    state.slots.remove(id);
                    log::info!("Removed dead process tracking entry for instance {}", id);
                    events.push((id.clone(), InstanceState::Stopped));
                }
            }
            MonitorOutcome::Alive {
                id,
                health_failure_count,
                next_health_check_at,
                alive_failure_count,
                next_alive_check_at,
                new_pid,
            } => {
                if let Some(InstanceEntry {
                    slot: Some(Slot::Live(p)),
                    ..
                }) = state.slots.get_mut(id)
                {
                    let old_state = derive_health_state(p);

                    p.health_failure_count = *health_failure_count;
                    p.next_health_check_at = *next_health_check_at;
                    p.alive_failure_count = *alive_failure_count;
                    p.next_alive_check_at = *next_alive_check_at;
                    if let Some(new_pid) = new_pid {
                        log::info!(
                            "Instance {} PID updated: {} -> {} (port {})",
                            id,
                            p.pid,
                            new_pid,
                            p.port
                        );
                        p.pid = *new_pid;
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
        let backoff = super::calculate_backoff(new_count);
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

// -- Liveness evaluation ------------------------------------------------------

/// Evaluate liveness for a single instance. Returns `None` if the process is
/// dead (slot should be removed), `Some(MonitorOutcome::Alive { .. })` if alive.
///
/// Platform-specific probing is delegated to [`probe_liveness`], which returns
/// `None` for terminal death or `Some((alive_failure_count, next_alive_check_at,
/// new_pid))` when the process is alive or retriable.
fn evaluate_liveness(entry: &LiveSnapshot, now: Instant) -> Option<MonitorOutcome> {
    // Backoff: not yet time to probe — preserve previous counters.
    // On non-Windows this is always a no-op (next_alive_check_at is always None).
    if entry.dashboard_enabled {
        if let Some(next_at) = entry.next_alive_check_at {
            if now < next_at {
                return Some(MonitorOutcome::Alive {
                    id: entry.id.clone(),
                    health_failure_count: entry.health_failure_count,
                    next_health_check_at: entry.next_health_check_at,
                    alive_failure_count: entry.alive_failure_count,
                    next_alive_check_at: entry.next_alive_check_at,
                    new_pid: None,
                });
            }
        }
    }

    let (alive_failure_count, next_alive_check_at, new_pid) = probe_liveness(entry, now)?;

    // If dashboard disabled and we are in retry mode, treat as dead.
    if !entry.dashboard_enabled && next_alive_check_at.is_some() {
        return None;
    }

    Some(MonitorOutcome::Alive {
        id: entry.id.clone(),
        health_failure_count: entry.health_failure_count,
        next_health_check_at: entry.next_health_check_at,
        alive_failure_count,
        next_alive_check_at,
        new_pid,
    })
}

// -- Platform-specific liveness probing ---------------------------------------

/// Returns `None` for terminal death, or `Some((alive_failure_count,
/// next_alive_check_at, new_pid))` when the process is alive or retriable.
#[cfg(target_os = "windows")]
fn probe_liveness(
    entry: &LiveSnapshot,
    now: Instant,
) -> Option<(u32, Option<Instant>, Option<u32>)> {
    if is_expected_process_alive(entry.pid, &entry.executable_path) {
        return Some((0, None, None));
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
            return Some((0, None, Some(new_pid)));
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
        None
    } else {
        let backoff = super::calculate_backoff(new_count);
        log::debug!(
            "Instance {} liveness probe failed (count: {}), retry in {}s",
            entry.id,
            new_count,
            backoff.as_secs()
        );
        Some((new_count, Some(now + backoff), None))
    }
}

#[cfg(not(target_os = "windows"))]
fn probe_liveness(
    entry: &LiveSnapshot,
    _now: Instant,
) -> Option<(u32, Option<Instant>, Option<u32>)> {
    if is_expected_process_alive(entry.pid, &entry.executable_path) {
        Some((0, None, None))
    } else {
        None
    }
}
