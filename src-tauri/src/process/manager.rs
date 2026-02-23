//! Instance process tracking and state management.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;
use std::time::Duration;

use reqwest::Client;
use tokio::sync::broadcast;

use crate::utils::sync::{read_lock_recover, write_lock_recover};

use super::control::graceful_shutdown;
use super::{
    InstanceProcess, InstanceRuntimeSnapshot, InstanceState, RuntimeEvent,
};

/// Manages running instance processes.
pub struct ProcessManager {
    pub(super) processes: RwLock<HashMap<String, InstanceProcess>>,
    pub(super) http_client: Client,
    pub(super) runtime_events: broadcast::Sender<RuntimeEvent>,
}

impl ProcessManager {
    #[allow(clippy::expect_used)]
    pub fn new() -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(3))
            .no_proxy()
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

    pub(super) fn emit_runtime_event(&self, instance_id: &str, state: InstanceState) {
        let _ = self.runtime_events.send(RuntimeEvent {
            instance_id: instance_id.to_string(),
            state,
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

    /// Update the state of a tracked instance.
    pub fn set_state(&self, instance_id: &str, state: InstanceState) {
        let mut procs = write_lock_recover(&self.processes, "ProcessManager.processes");
        if let Some(info) = procs.get_mut(instance_id) {
            info.state = state;
        }
    }

    /// Get a lightweight runtime snapshot for all tracked instances.
    ///
    /// This is a pure read — it returns the last state computed by the
    /// background monitor without triggering a new evaluation cycle.
    pub fn get_runtime_snapshot(&self) -> HashMap<String, InstanceRuntimeSnapshot> {
        let procs = read_lock_recover(&self.processes, "ProcessManager.processes");
        procs
            .iter()
            .map(|(id, info)| {
                (
                    id.clone(),
                    InstanceRuntimeSnapshot {
                        state: info.state,
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
