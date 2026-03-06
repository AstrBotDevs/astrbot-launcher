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
use super::UNHEALTHY_THRESHOLD;
use super::{InstanceProcess, InstanceRuntimeInfo, InstanceState, RuntimeEvent};
use crate::backup::find_pending_auto_backup;
use crate::error::{AppError, ErrorKind, Result};
use crate::instance::lifecycle;
use crate::utils::sync::lock_mutex_recover;

/// A single slot in the process state machine.
///
/// Each instance occupies at most one slot. The variant encodes the lifecycle
/// phase; CRUD guard tracking lives alongside in [`InstanceEntry`].
pub(super) enum Slot {
    /// Launch IO in progress.
    Starting,
    /// Process running (healthy or unhealthy, derived from `InstanceProcess`).
    Live(InstanceProcess),
    /// Shutdown IO in progress. Keeps the same process info so the exit
    /// handler can force-kill processes that are still shutting down.
    Stopping(InstanceProcess),
}

/// Per-instance entry combining lifecycle slot and CRUD guard state.
///
/// An entry with `slot: None, guarded: true` represents a stopped instance
/// undergoing a CRUD operation.  The entry is removed when the guard drops.
pub(super) struct InstanceEntry {
    pub(super) slot: Option<Slot>,
    guarded: bool,
}

pub(super) struct ProcessState {
    pub(super) slots: HashMap<String, InstanceEntry>,
    pub(super) shutting_down: bool,
}

impl ProcessState {
    /// Reject if the application is shutting down.
    fn check_active(&self, id: &str) -> Result<()> {
        if self.shutting_down {
            return Err(AppError::process(format!(
                "Cannot operate on instance {id}: application is shutting down"
            )));
        }
        Ok(())
    }

    /// Reject if `id` is guarded or already occupies a lifecycle slot.
    fn ensure_vacant(&self, id: &str) -> Result<()> {
        match self.slots.get(id) {
            None => Ok(()),
            Some(entry) if entry.guarded => Err(AppError::process(format!(
                "Instance {id}: another operation is in progress"
            ))),
            Some(_) => Err(AppError::instance_running()),
        }
    }

    /// Transition a `Live` slot to `Stopping`, returning the pid and exe path.
    ///
    /// Returns an appropriate error for every non-`Live` state.
    fn prepare_stop(&mut self, id: &str) -> Result<(u32, PathBuf)> {
        let entry = match self.slots.get_mut(id) {
            None => return Err(AppError::instance_not_running()),
            Some(e) => e,
        };

        if entry.guarded {
            return Err(AppError::process(format!(
                "Instance {id}: another operation is in progress"
            )));
        }

        match entry.slot.take() {
            Some(Slot::Live(p)) => {
                let pid = p.pid;
                let exe = p.executable_path.clone();
                entry.slot = Some(Slot::Stopping(p));
                Ok((pid, exe))
            }
            Some(slot @ Slot::Stopping(_)) => {
                entry.slot = Some(slot);
                Err(AppError::process(format!(
                    "Instance {id} is already stopping"
                )))
            }
            Some(slot @ Slot::Starting) => {
                entry.slot = Some(slot);
                Err(AppError::process(format!(
                    "Instance {id} is still starting"
                )))
            }
            None => Err(AppError::instance_not_running()),
        }
    }

    /// Revert a `Stopping` slot back to `Live`. Returns `true` if reverted.
    fn revert_stop(&mut self, id: &str) -> bool {
        if let Some(entry) = self.slots.get_mut(id) {
            if let Some(Slot::Stopping(p)) = entry.slot.take() {
                entry.slot = Some(Slot::Live(p));
                return true;
            }
        }
        false
    }

    /// Drain all slots for application shutdown and collect killable processes.
    ///
    /// Sets `shutting_down = true`, drains every entry, and returns
    /// `(id, InstanceProcess)` for `Live`/`Stopping` entries.
    fn drain_for_shutdown(&mut self) -> Vec<(String, InstanceProcess)> {
        self.shutting_down = true;

        self.slots
            .drain()
            .filter_map(|(id, entry)| match entry.slot {
                Some(Slot::Live(p)) | Some(Slot::Stopping(p)) => Some((id, p)),
                Some(Slot::Starting) => {
                    log::info!(
                        "Cleared Starting slot for instance {} during shutdown \
                         (async task will handle cleanup)",
                        id
                    );
                    None
                }
                None => None,
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
    http_client: Option<Client>,
}

impl ProcessManager {
    pub fn new() -> Self {
        let (runtime_events, _) = broadcast::channel(128);
        let state = Arc::new(Mutex::new(ProcessState {
            slots: HashMap::new(),
            shutting_down: false,
        }));

        let http_client = Client::builder()
            .timeout(Duration::from_secs(3))
            .no_proxy()
            .build()
            .map_err(|e| {
                log::error!(
                    "Failed to create monitor HTTP client: {}. \
                     Health checks will be degraded to liveness-only mode.",
                    e
                );
            })
            .ok();

        Self {
            state,
            runtime_events,
            http_client,
        }
    }

    /// Spawn the background monitor task that periodically polls all instances.
    ///
    /// Must be called after the Tauri async runtime is available (e.g. in `setup`).
    pub fn start_monitor(&self) {
        let monitor_state = Arc::clone(&self.state);
        let monitor_events = self.runtime_events.clone();
        let http_client = self.http_client.clone();
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(super::MONITOR_INTERVAL);

            loop {
                interval.tick().await;
                monitor::poll_instances(&monitor_state, http_client.as_ref(), &monitor_events)
                    .await;
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
            Some(InstanceEntry {
                slot: Some(Slot::Live(p)),
                ..
            }) => Some(p.port),
            _ => None,
        }
    }

    pub fn get_runtime_info(&self) -> HashMap<String, InstanceRuntimeInfo> {
        let state = lock_mutex_recover(&self.state, "ProcessState");
        state
            .slots
            .iter()
            .filter_map(|(id, entry)| {
                let info = match &entry.slot {
                    Some(Slot::Starting) => InstanceRuntimeInfo::Starting,
                    Some(Slot::Live(p)) => InstanceRuntimeInfo::Live {
                        state: derive_health_state(p),
                        port: p.port,
                        dashboard_enabled: p.dashboard_enabled,
                    },
                    Some(Slot::Stopping(p)) => InstanceRuntimeInfo::Stopping {
                        port: p.port,
                        dashboard_enabled: p.dashboard_enabled,
                    },
                    None => return None,
                };
                Some((id.clone(), info))
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
            .filter(|(_, entry)| matches!(&entry.slot, Some(Slot::Live(_)) | Some(Slot::Starting)))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Acquire a guard that prevents lifecycle operations on the instance.
    /// The guard is released when dropped.
    pub fn acquire_guard(&self, id: &str) -> Result<InstanceGuard> {
        let mut state = lock_mutex_recover(&self.state, "ProcessState");
        state.check_active(id)?;
        state.ensure_vacant(id)?;
        state.slots.insert(
            id.to_string(),
            InstanceEntry {
                slot: None,
                guarded: true,
            },
        );
        drop(state);
        Ok(InstanceGuard {
            instance_id: id.to_string(),
            state: Arc::clone(&self.state),
        })
    }

    // -- Async methods (involve IO) -------------------------------------------

    pub async fn start_instance(&self, id: &str, app_handle: tauri::AppHandle) -> Result<u16> {
        // Check for pending auto backup before attempting to start
        let pending_auto_backup = find_pending_auto_backup(id)?;
        if let Some(pending_auto_backup) = pending_auto_backup {
            return Err(AppError::backup(format!(
                "检测到未处理的自动备份 {}（{}）。这通常表示上次升降级的数据还原失败。请先在\"备份管理\"中手动恢复该备份并确认数据，再启动实例。",
                pending_auto_backup.filename, pending_auto_backup.path
            )));
        }

        // Phase 1: lock → check → set Starting → unlock
        {
            let mut state = lock_mutex_recover(&self.state, "ProcessState");
            state.check_active(id)?;
            state.ensure_vacant(id)?;
            state.slots.insert(
                id.to_string(),
                InstanceEntry {
                    slot: Some(Slot::Starting),
                    guarded: false,
                },
            );
        }
        self.emit(id, InstanceState::Starting);

        // Phase 2+3: launch IO and finalize slot
        self.launch_and_finalize(id, &app_handle).await
    }

    pub async fn stop_instance(&self, id: &str) -> Result<()> {
        // Phase 1: lock → transition to Stopping → unlock
        let (pid, exe) = {
            let mut state = lock_mutex_recover(&self.state, "ProcessState");
            state.check_active(id)?;
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
        // Check for pending auto backup before attempting to restart
        let pending_auto_backup = find_pending_auto_backup(id)?;
        if let Some(pending_auto_backup) = pending_auto_backup {
            return Err(AppError::backup(format!(
                "检测到未处理的自动备份 {}（{}）。这通常表示上次升降级的数据还原失败。请先在\"备份管理\"中手动恢复该备份并确认数据，再启动实例。",
                pending_auto_backup.filename, pending_auto_backup.path
            )));
        }

        // Phase 1: lock → check → prepare stop if running, or insert Starting directly
        let stop_info = {
            let mut state = lock_mutex_recover(&self.state, "ProcessState");
            state.check_active(id)?;
            match state.prepare_stop(id) {
                Ok((pid, exe)) => Some((pid, exe)),
                Err(e) if e.kind() == ErrorKind::InstanceNotRunning => {
                    // Not running — go straight to Starting.
                    state.slots.insert(
                        id.to_string(),
                        InstanceEntry {
                            slot: Some(Slot::Starting),
                            guarded: false,
                        },
                    );
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
                    return Err(AppError::process(format!(
                        "Cannot restart instance {id}: application is shutting down"
                    )));
                }
                if let Some(entry) = state.slots.get_mut(id) {
                    entry.slot = Some(Slot::Starting);
                }
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
            return Err(AppError::process(format!(
                "Cannot start instance {id}: application is shutting down"
            )));
        }

        match result {
            Ok(launch) => {
                let port = launch.port;
                if let Some(entry) = state.slots.get_mut(id) {
                    entry.slot = Some(Slot::Live(InstanceProcess::new(
                        launch.pid,
                        launch.executable_path,
                        launch.port,
                        launch.dashboard_enabled,
                    )));
                }
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
        let entries = {
            let mut state = lock_mutex_recover(&self.state, "ProcessState");
            state.drain_for_shutdown()
        };

        if entries.is_empty() {
            return;
        }

        for (id, p) in &entries {
            log::info!(
                "Stopping instance {} (pid: {}, port: {})",
                id,
                p.pid,
                p.port
            );
        }

        let target_refs: Vec<(u32, &std::path::Path)> = entries
            .iter()
            .map(|(_, p)| (p.pid, p.executable_path.as_path()))
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
        let mut state = lock_mutex_recover(&self.state, "ProcessState");
        // Remove the guard-only entry. If shutdown drained it, this is a no-op.
        if matches!(
            state.slots.get(&self.instance_id),
            Some(InstanceEntry {
                slot: None,
                guarded: true
            })
        ) {
            state.slots.remove(&self.instance_id);
        }
    }
}
