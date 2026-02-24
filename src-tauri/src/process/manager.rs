//! Process manager — `Mutex<ProcessState>` approach.
//!
//! Sync methods for queries (direct lock → read → unlock).
//! Async methods for lifecycle operations that involve IO.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::Client;
use tokio::sync::broadcast;

use super::control::graceful_shutdown;
use super::monitor;
use super::{InstanceProcess, InstanceRuntimeInfo, InstanceState, RuntimeEvent};
use super::UNHEALTHY_THRESHOLD;
use crate::error::{AppError, ErrorKind, Result};
use crate::instance::lifecycle;
use crate::utils::sync::lock_mutex_recover;

/// A single slot in the process state machine.
///
/// Each instance occupies at most one slot. The variant encodes the lifecycle
/// phase. Coordination guards are tracked separately in [`ProcessState::guarded`].
pub(super) enum Slot {
    /// Launch IO in progress.
    Starting,
    /// Process running (healthy or unhealthy, derived from `InstanceProcess`).
    Live(InstanceProcess),
    /// Shutdown IO in progress. Keeps the same process info so the exit
    /// handler can force-kill processes that are still shutting down.
    Stopping(InstanceProcess),
}

pub(super) struct ProcessState {
    pub(super) slots: HashMap<String, Slot>,
    pub(super) shutting_down: bool,
    /// Instance IDs with an active CRUD guard. Tracked separately from lifecycle
    /// slots so that `Slot` only describes process states.
    guarded: HashSet<String>,
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

    /// Reject if `id` is guarded or already occupies a lifecycle slot.
    fn check_slot_vacant(&self, id: &str) -> Result<()> {
        if self.guarded.contains(id) {
            return Err(AppError::process("Another operation is in progress"));
        }
        match self.slots.get(id) {
            None => Ok(()),
            Some(_) => Err(AppError::instance_running()),
        }
    }

    /// Transition a `Live` slot to `Stopping`, returning the pid and exe path.
    ///
    /// Returns an appropriate error for every non-`Live` state.
    fn prepare_stop(&mut self, id: &str) -> Result<(u32, PathBuf)> {
        if self.guarded.contains(id) {
            return Err(AppError::process("Another operation is in progress"));
        }
        match self.slots.remove(id) {
            Some(Slot::Live(p)) => {
                let pid = p.pid;
                let exe = p.executable_path.clone();
                self.slots.insert(id.to_string(), Slot::Stopping(p));
                Ok((pid, exe))
            }
            Some(slot @ Slot::Stopping(_)) => {
                self.slots.insert(id.to_string(), slot);
                Err(AppError::process("Instance is already stopping"))
            }
            Some(slot @ Slot::Starting) => {
                self.slots.insert(id.to_string(), slot);
                Err(AppError::process("Instance is still starting"))
            }
            None => Err(AppError::instance_not_running()),
        }
    }

    /// Revert a `Stopping` slot back to `Live`. Returns `true` if reverted.
    fn revert_stop(&mut self, id: &str) -> bool {
        if let Some(Slot::Stopping(p)) = self.slots.remove(id) {
            self.slots.insert(id.to_string(), Slot::Live(p));
            true
        } else {
            false
        }
    }

    /// Drain all slots for application shutdown and collect killable targets.
    ///
    /// Sets `shutting_down = true`, invalidates all guards, drains every slot,
    /// and returns `ShutdownTarget` for `Live`/`Stopping` entries.
    fn drain_for_shutdown(&mut self) -> Vec<ShutdownTarget> {
        self.shutting_down = true;
        self.guarded.clear();

        self.slots
            .drain()
            .filter_map(|(id, slot)| match slot {
                Slot::Live(p) | Slot::Stopping(p) => Some(ShutdownTarget {
                    id,
                    pid: p.pid,
                    port: p.port,
                    executable_path: p.executable_path,
                }),
                Slot::Starting => {
                    log::info!(
                        "Cleared Starting slot for instance {} during shutdown \
                         (async task will handle cleanup)",
                        id
                    );
                    None
                }
            })
            .collect()
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
            guarded: HashSet::new(),
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
            let mut interval = tokio::time::interval(super::MONITOR_INTERVAL);
            let mut http_client: Option<Client> = None;

            loop {
                interval.tick().await;

                if http_client.is_none() {
                    match Client::builder()
                        .timeout(Duration::from_secs(3))
                        .no_proxy()
                        .build()
                    {
                        Ok(client) => http_client = Some(client),
                        Err(e) => {
                            log::error!(
                                "Failed to create monitor HTTP client: {e}; retrying next tick"
                            );
                            continue;
                        }
                    }
                }

                if let Some(client) = http_client.as_ref() {
                    monitor::poll_instances(&monitor_state, client, &monitor_events).await;
                }
            }
        });
    }

    pub fn subscribe_runtime_events(&self) -> broadcast::Receiver<RuntimeEvent> {
        self.runtime_events.subscribe()
    }

    // -- Sync methods (direct lock → read/write → unlock) ---------------------

    /// Returns the port for a **live** instance, or `None` if the instance is
    /// not in the `Live` state.
    ///
    /// Ports are not available during `Starting` or other transient states
    /// because the actual port is only known after launch completes.
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
            .map(|(id, slot)| {
                let info = match slot {
                    Slot::Starting => InstanceRuntimeInfo::Starting,
                    Slot::Live(p) => InstanceRuntimeInfo::Live {
                        state: derive_health_state(p),
                        port: p.port,
                        dashboard_enabled: p.dashboard_enabled,
                    },
                    Slot::Stopping(p) => InstanceRuntimeInfo::Stopping {
                        port: p.port,
                        dashboard_enabled: p.dashboard_enabled,
                    },
                };
                (id.clone(), info)
            })
            .collect()
    }

    /// Returns IDs of instances that are either live or starting.
    ///
    /// Used to persist in-progress instance IDs across application restarts.
    pub fn get_active_ids(&self) -> Vec<String> {
        let state = lock_mutex_recover(&self.state, "ProcessState");
        state
            .slots
            .iter()
            .filter(|(_, slot)| matches!(slot, Slot::Live(_) | Slot::Starting))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Acquire a guard that prevents lifecycle operations on the instance.
    /// The guard is released when dropped.
    pub fn acquire_guard(&self, id: &str) -> Result<InstanceGuard> {
        let mut state = lock_mutex_recover(&self.state, "ProcessState");
        state.check_active()?;
        state.check_slot_vacant(id)?;
        state.guarded.insert(id.to_string());
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
        // Phase 1: lock → transition to Stopping → unlock
        let (pid, exe) = {
            let mut state = lock_mutex_recover(&self.state, "ProcessState");
            state.check_active()?;
            state.prepare_stop(id)?
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
            match state.prepare_stop(id) {
                Ok((pid, exe)) => Some((pid, exe)),
                Err(e) if e.kind() == ErrorKind::InstanceNotRunning => {
                    // Not running — go straight to Starting.
                    state.slots.insert(id.to_string(), Slot::Starting);
                    None
                }
                Err(e) => return Err(e),
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
        let result = lifecycle::launch_instance(id, app_handle).await;

        let mut state = lock_mutex_recover(&self.state, "ProcessState");

        // If shutting down, kill any late-arriving process to prevent orphans.
        if state.shutting_down {
            state.slots.remove(id);
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
                drop(state);
                self.emit(id, InstanceState::Running);
                Ok(port)
            }
            Err(e) => {
                state.slots.remove(id);
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
            .guarded
            .remove(&self.instance_id);
    }
}
