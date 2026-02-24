//! Process manager — `Mutex<ProcessState>` approach.
//!
//! Sync methods for queries (direct lock → read → unlock).
//! Async methods for lifecycle operations that involve IO.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::Client;
use tokio::sync::broadcast;

use super::control::graceful_shutdown;
use super::monitor;
use super::{InstanceProcess, InstanceRuntimeInfo, InstanceState, RuntimeEvent};
use super::UNHEALTHY_THRESHOLD;
use crate::error::{AppError, Result};
use crate::instance::lifecycle;
use crate::utils::sync::lock_mutex_recover;

/// Data-free discriminant of [`Slot`], used as a filter key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    Guarded,
    Starting,
    Live,
    Stopping,
}

/// A single slot in the process state machine.
///
/// Each instance occupies at most one slot. The variant encodes the lifecycle
/// phase, eliminating the need for separate collections.
pub(super) enum Slot {
    /// CRUD protection active (guard held).
    Guarded,
    /// Launch IO in progress.
    Starting,
    /// Process running (healthy or unhealthy, derived from `InstanceProcess`).
    Live(InstanceProcess),
    /// Shutdown IO in progress. Retains pid/exe so the exit handler can
    /// force-kill processes that are still shutting down.
    Stopping {
        pid: u32,
        executable_path: PathBuf,
        port: u16,
        dashboard_enabled: bool,
    },
}

impl Slot {
    pub(super) fn kind(&self) -> SlotKind {
        match self {
            Self::Guarded => SlotKind::Guarded,
            Self::Starting => SlotKind::Starting,
            Self::Live(_) => SlotKind::Live,
            Self::Stopping { .. } => SlotKind::Stopping,
        }
    }
}

pub(super) struct ProcessState {
    pub(super) slots: HashMap<String, Slot>,
    pub(super) shutting_down: bool,
}

/// Info needed to shut down one instance during app exit.
struct ShutdownTarget {
    id: String,
    pid: u32,
    port: u16,
    executable_path: PathBuf,
}

impl ProcessState {
    /// Reject if the application is shutting down.
    fn check_active(&self) -> Result<()> {
        if self.shutting_down {
            return Err(AppError::process("Application shutting down"));
        }
        Ok(())
    }

    /// Reject if `id` already occupies a slot.
    ///
    /// - `None` → Ok (vacant)
    /// - `Guarded` → "Another operation is in progress"
    /// - any other → instance_running
    fn check_slot_vacant(&self, id: &str) -> Result<()> {
        match self.slots.get(id) {
            None => Ok(()),
            Some(Slot::Guarded) => Err(AppError::process("Another operation is in progress")),
            Some(_) => Err(AppError::instance_running()),
        }
    }

    /// Transition a `Live` slot to `Stopping`, returning the pid and exe path.
    ///
    /// Returns `None` if the slot is not `Live` (caller decides the error).
    fn begin_stop(&mut self, id: &str) -> Option<(u32, PathBuf)> {
        if let Some(Slot::Live(p)) = self.slots.get(id) {
            let pid = p.pid;
            let exe = p.executable_path.clone();
            let port = p.port;
            let dashboard_enabled = p.dashboard_enabled;
            self.slots.insert(
                id.to_string(),
                Slot::Stopping {
                    pid,
                    executable_path: exe.clone(),
                    port,
                    dashboard_enabled,
                },
            );
            Some((pid, exe))
        } else {
            None
        }
    }

    /// Revert a `Stopping` slot back to `Live`. Returns `true` if reverted.
    fn revert_stop(&mut self, id: &str) -> bool {
        if let Some(Slot::Stopping {
            pid,
            executable_path,
            port,
            dashboard_enabled,
        }) = self.slots.remove(id)
        {
            self.slots.insert(
                id.to_string(),
                Slot::Live(InstanceProcess::new(pid, executable_path, port, dashboard_enabled)),
            );
            true
        } else {
            false
        }
    }

    /// Drain all slots for application shutdown and collect killable targets.
    ///
    /// Sets `shutting_down = true`, drains every slot, and returns
    /// `ShutdownTarget` for `Live`/`Stopping` entries. `Starting`/`Guarded`
    /// slots are logged and discarded.
    fn drain_for_shutdown(&mut self) -> Vec<ShutdownTarget> {
        self.shutting_down = true;

        let all_slots: Vec<(String, Slot)> = self.slots.drain().collect();
        let mut targets = Vec::new();

        for (id, slot) in all_slots {
            match slot {
                Slot::Live(p) => {
                    targets.push(ShutdownTarget {
                        id,
                        pid: p.pid,
                        port: p.port,
                        executable_path: p.executable_path,
                    });
                }
                Slot::Stopping {
                    pid,
                    executable_path,
                    port,
                    ..
                } => {
                    targets.push(ShutdownTarget {
                        id,
                        pid,
                        port,
                        executable_path,
                    });
                }
                Slot::Starting => {
                    log::info!(
                        "Cleared Starting slot for instance {} during shutdown \
                         (async task will handle cleanup)",
                        id
                    );
                }
                Slot::Guarded => {
                    log::debug!(
                        "Cleared Guarded slot for instance {} during shutdown",
                        id
                    );
                }
            }
        }

        targets
    }
}

/// Derive the health state from an `InstanceProcess` without storing it.
pub(super) fn derive_health_state(p: &InstanceProcess) -> InstanceState {
    if p.health_failure_count >= UNHEALTHY_THRESHOLD {
        InstanceState::Unhealthy
    } else {
        InstanceState::Running
    }
}

/// Manages running instance processes via a shared mutex-guarded state.
#[derive(Clone)]
pub struct ProcessManager {
    state: Arc<Mutex<ProcessState>>,
    runtime_events: broadcast::Sender<RuntimeEvent>,
}

impl ProcessManager {
    pub fn new() -> Self {
        let (runtime_events, _) = broadcast::channel(128);
        let state = Arc::new(Mutex::new(ProcessState {
            slots: HashMap::new(),
            shutting_down: false,
        }));

        Self {
            state,
            runtime_events,
        }
    }

    /// Spawn the background monitor task that periodically polls all instances.
    ///
    /// Must be called after the Tauri async runtime is available (e.g. in `setup`).
    pub fn start_monitor(&self) {
        let monitor_state = Arc::clone(&self.state);
        let monitor_events = self.runtime_events.clone();
        tauri::async_runtime::spawn(async move {
            let http_client = match Client::builder()
                .timeout(Duration::from_secs(3))
                .no_proxy()
                .build()
            {
                Ok(client) => client,
                Err(e) => {
                    log::error!("Failed to create monitor HTTP client: {e}; monitor will not run");
                    return;
                }
            };

            let mut interval = tokio::time::interval(super::MONITOR_INTERVAL);
            loop {
                interval.tick().await;
                monitor::poll_instances(&monitor_state, &http_client, &monitor_events).await;
            }
        });
    }

    pub fn subscribe_runtime_events(&self) -> broadcast::Receiver<RuntimeEvent> {
        self.runtime_events.subscribe()
    }

    // -- Sync methods (direct lock → read/write → unlock) ---------------------

    pub fn get_port(&self, id: &str) -> Option<u16> {
        let state = lock_mutex_recover(&self.state, "ProcessState");
        match state.slots.get(id) {
            Some(Slot::Live(p)) => Some(p.port),
            _ => None,
        }
    }

    pub fn get_runtime_info(&self) -> HashMap<String, InstanceRuntimeInfo> {
        let state = lock_mutex_recover(&self.state, "ProcessState");
        state
            .slots
            .iter()
            .filter_map(|(id, slot)| {
                let info = match slot {
                    Slot::Starting => InstanceRuntimeInfo::Starting,
                    Slot::Live(p) => InstanceRuntimeInfo::Live {
                        state: derive_health_state(p),
                        port: p.port,
                        dashboard_enabled: p.dashboard_enabled,
                    },
                    Slot::Stopping {
                        port,
                        dashboard_enabled,
                        ..
                    } => InstanceRuntimeInfo::Stopping {
                        port: *port,
                        dashboard_enabled: *dashboard_enabled,
                    },
                    // Guard is a CRUD lock, not a user-visible state.
                    Slot::Guarded => return None,
                };
                Some((id.clone(), info))
            })
            .collect()
    }

    pub fn get_tracked_ids(&self, kinds: &[SlotKind]) -> Vec<String> {
        let state = lock_mutex_recover(&self.state, "ProcessState");
        state
            .slots
            .iter()
            .filter_map(|(id, slot)| kinds.contains(&slot.kind()).then(|| id.clone()))
            .collect()
    }

    /// Acquire a guard that prevents lifecycle operations on the instance.
    /// The guard is released when dropped.
    pub fn acquire_guard(&self, id: &str) -> Result<InstanceGuard> {
        let mut state = lock_mutex_recover(&self.state, "ProcessState");
        state.check_active()?;
        state.check_slot_vacant(id)?;
        state.slots.insert(id.to_string(), Slot::Guarded);
        drop(state);
        Ok(InstanceGuard {
            instance_id: id.to_string(),
            state: Arc::clone(&self.state),
        })
    }

    // -- Async methods (involve IO) -------------------------------------------

    pub async fn start_instance(&self, id: &str, app_handle: tauri::AppHandle) -> Result<u16> {
        // Phase 1: lock → check → set Starting → unlock
        {
            let mut state = lock_mutex_recover(&self.state, "ProcessState");
            state.check_active()?;
            state.check_slot_vacant(id)?;
            state.slots.insert(id.to_string(), Slot::Starting);
        }
        self.emit(id, InstanceState::Starting);

        // Phase 2+3: launch IO and finalize slot
        self.launch_and_finalize(id, &app_handle).await
    }

    pub async fn stop_instance(&self, id: &str) -> Result<()> {
        // Phase 1: lock → validate → replace with Stopping → extract info → unlock
        let (pid, exe) = {
            let mut state = lock_mutex_recover(&self.state, "ProcessState");
            state.check_active()?;
            match state.begin_stop(id) {
                Some(info) => info,
                None => {
                    return match state.slots.get(id).map(Slot::kind) {
                        Some(SlotKind::Stopping) => {
                            Err(AppError::process("Instance is already stopping"))
                        }
                        Some(SlotKind::Starting) => {
                            Err(AppError::process("Instance is still starting"))
                        }
                        Some(SlotKind::Guarded) => {
                            Err(AppError::process("Another operation is in progress"))
                        }
                        None => Err(AppError::instance_not_running()),
                        _ => unreachable!(),
                    };
                }
            }
        };
        self.emit(id, InstanceState::Stopping);

        // Phase 2: shutdown IO (no lock held)
        if let Err(e) = lifecycle::shutdown_instance(pid, exe).await {
            self.revert_stopping_to_live(id);
            return Err(e);
        }

        // Phase 3: finalize — remove slot and emit Stopped.
        self.finalize_stop(id);
        Ok(())
    }

    pub async fn restart_instance(&self, id: &str, app_handle: tauri::AppHandle) -> Result<u16> {
        // Phase 1: lock → check → prepare stop if running, or insert Starting directly
        let stop_info = {
            let mut state = lock_mutex_recover(&self.state, "ProcessState");
            state.check_active()?;
            match state.begin_stop(id) {
                Some(info) => Some(info),
                None => match state.slots.get(id).map(Slot::kind) {
                    Some(SlotKind::Starting) => return Err(AppError::instance_running()),
                    Some(SlotKind::Guarded) => {
                        return Err(AppError::process("Another operation is in progress"));
                    }
                    Some(SlotKind::Stopping) => {
                        return Err(AppError::process("Instance is already stopping"));
                    }
                    None => {
                        // Not running — go straight to Starting.
                        state.slots.insert(id.to_string(), Slot::Starting);
                        None
                    }
                    _ => unreachable!(),
                },
            }
        };

        // Phase 2: stop if was running, then atomically transition Stopping → Starting
        if let Some((pid, exe)) = stop_info {
            self.emit(id, InstanceState::Stopping);
            if let Err(e) = lifecycle::shutdown_instance(pid, exe).await {
                log::error!("Instance {} shutdown failed during restart: {}", id, e);
                self.revert_stopping_to_live(id);
                return Err(e);
            }
            // Atomically transition Stopping → Starting (no unprotected gap).
            {
                let mut state = lock_mutex_recover(&self.state, "ProcessState");
                if state.shutting_down {
                    state.slots.remove(id);
                    drop(state);
                    self.emit(id, InstanceState::Stopped);
                    return Err(AppError::process("Application shutting down"));
                }
                state.slots.insert(id.to_string(), Slot::Starting);
            }
        }

        // Skip emitting Stopped to avoid UI flicker — go directly to Starting.
        self.emit(id, InstanceState::Starting);

        // Phase 3: launch IO and finalize slot
        self.launch_and_finalize(id, &app_handle).await
    }

    /// Run launch IO and finalize the slot.
    ///
    /// Precondition: the caller has already inserted `Slot::Starting` for `id`.
    async fn launch_and_finalize(&self, id: &str, app_handle: &tauri::AppHandle) -> Result<u16> {
        let mut slot_guard = StartingSlotGuard {
            instance_id: id.to_string(),
            state: Arc::clone(&self.state),
            defused: false,
        };

        let result = lifecycle::launch_instance(id, app_handle).await;

        let mut state = lock_mutex_recover(&self.state, "ProcessState");

        // If shutting down, kill any late-arriving process to prevent orphans.
        if state.shutting_down {
            state.slots.remove(id);
            slot_guard.defused = true;
            drop(state);
            if let Ok(launch) = result {
                log::info!(
                    "Killing late-started instance {} (pid: {}) due to shutdown",
                    id,
                    launch.pid
                );
                let pid = launch.pid;
                let exe = launch.executable_path;
                let late_id = id.to_string();
                tokio::spawn(async move {
                    if let Err(e) = lifecycle::shutdown_instance(pid, exe).await {
                        log::warn!("Failed to kill late-started instance {}: {}", late_id, e);
                    }
                });
            }
            return Err(AppError::process("Application shutting down"));
        }

        match result {
            Ok(launch) => {
                let port = launch.port;
                state.slots.insert(
                    id.to_string(),
                    Slot::Live(InstanceProcess::new(
                        launch.pid,
                        launch.executable_path,
                        launch.port,
                        launch.dashboard_enabled,
                    )),
                );
                slot_guard.defused = true;
                drop(state);
                self.emit(id, InstanceState::Running);
                Ok(port)
            }
            Err(e) => {
                state.slots.remove(id);
                slot_guard.defused = true;
                drop(state);
                self.emit(id, InstanceState::Stopped);
                Err(e)
            }
        }
    }

    /// Blocking shutdown for the exit handler.
    ///
    /// Called from the Tauri `RunEvent::Exit` handler (not inside async context).
    /// Drains ALL slots and force-kills every instance that has a known PID.
    pub fn stop_all_blocking(&self) {
        let targets = {
            let mut state = lock_mutex_recover(&self.state, "ProcessState");
            state.drain_for_shutdown()
        };

        if targets.is_empty() {
            return;
        }

        for t in &targets {
            log::info!("Stopping instance {} (pid: {}, port: {})", t.id, t.pid, t.port);
        }

        let target_refs: Vec<(u32, &std::path::Path)> = targets
            .iter()
            .map(|t| (t.pid, t.executable_path.as_path()))
            .collect();

        graceful_shutdown(&target_refs);
    }

    /// Revert a `Stopping` slot back to `Live` after a failed shutdown,
    /// so the user can retry. Emits `Running` if the slot was restored.
    fn revert_stopping_to_live(&self, id: &str) {
        let mut state = lock_mutex_recover(&self.state, "ProcessState");
        if state.revert_stop(id) {
            drop(state);
            self.emit(id, InstanceState::Running);
        }
    }

    /// Finalize a stop: remove the slot and emit Stopped.
    fn finalize_stop(&self, id: &str) {
        lock_mutex_recover(&self.state, "ProcessState")
            .slots
            .remove(id);
        self.emit(id, InstanceState::Stopped);
    }

    fn emit(&self, instance_id: &str, state: InstanceState) {
        let _ = self.runtime_events.send(RuntimeEvent {
            instance_id: instance_id.to_string(),
            state,
        });
    }
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard that prevents lifecycle operations on an instance.
/// Released automatically when dropped.
pub struct InstanceGuard {
    instance_id: String,
    state: Arc<Mutex<ProcessState>>,
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        lock_mutex_recover(&self.state, "ProcessState")
            .slots
            .remove(&self.instance_id);
    }
}

/// RAII guard that removes an orphaned `Slot::Starting` on panic or cancellation.
/// Defuse it on all normal exit paths where the slot is already cleaned up.
struct StartingSlotGuard {
    instance_id: String,
    state: Arc<Mutex<ProcessState>>,
    defused: bool,
}

impl Drop for StartingSlotGuard {
    fn drop(&mut self) {
        if !self.defused {
            lock_mutex_recover(&self.state, "ProcessState")
                .slots
                .remove(&self.instance_id);
        }
    }
}
