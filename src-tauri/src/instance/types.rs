//! Instance-related type definitions.

use serde::{Deserialize, Serialize};

use crate::process::InstanceState;

/// Status information for an instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceStatus {
    pub id: String,
    pub name: String,
    pub state: InstanceState,
    pub port: u16,
    pub version: String,
    pub dashboard_enabled: bool,
    pub pid_tracker_not_available: bool,
    pub configured_host: String,
    pub configured_port: u16,
}

/// Deployment progress event payload.
#[derive(Debug, Clone, Serialize)]
pub struct DeployProgress {
    pub instance_id: String,
    /// Step name: "extract", "venv", "deps", "start", "done", "error"
    pub step: String,
    pub message: String,
    /// Progress percentage: 0-100
    pub progress: u8,
}

/// Dashboard config from cmd_config.json.
#[derive(Debug, Deserialize)]
pub(crate) struct CmdConfigDashboard {
    pub enable: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CmdConfig {
    #[serde(default)]
    pub dashboard: Option<CmdConfigDashboard>,
}
