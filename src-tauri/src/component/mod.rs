mod common;
mod migration;
mod node_shim;
mod nodejs;
mod python;
mod registry;
mod types;
mod uv;

use std::env;
use std::ffi::OsString;
use std::path::Path;

use reqwest::Client;
use tauri::AppHandle;

use crate::error::{AppError, Result};
use crate::paths::{get_component_dir, get_node_exe_path, get_nodejs_shim_dir};

pub use common::normalize_default_index;
pub use migration::migrate_legacy_python_dirs;
pub use node_shim::generate_shims;
pub use nodejs::build_nodejs_env_vars;
pub use python::{create_venv, pip_install_requirements};
pub use types::{ComponentId, ComponentsSnapshot};
pub use uv::{is_uv_installed, uv_sync};

use types::ComponentStatus;

/// Build a snapshot of all component statuses.
pub fn build_components_snapshot() -> ComponentsSnapshot {
    let components = registry::COMPONENT_DESCRIPTORS
        .iter()
        .map(|descriptor| {
            let id = descriptor.id;
            ComponentStatus {
                id: id.dir_name().to_string(),
                installed: (descriptor.is_installed)(),
                display_name: id.display_name().to_string(),
                description: descriptor.description.to_string(),
            }
        })
        .collect();

    ComponentsSnapshot { components }
}

/// Install a component by id, dispatching to the appropriate sub-module.
pub async fn install_component(
    client: &Client,
    id: ComponentId,
    app_handle: Option<&AppHandle>,
) -> Result<String> {
    log::info!("Installing component {:?}", id);
    let result = match id {
        ComponentId::Python => python::install_component(client, app_handle).await,
        ComponentId::Nodejs => nodejs::install_nodejs(client, app_handle).await,
        ComponentId::UV => uv::install_uv(client, app_handle).await,
    };

    match &result {
        Ok(_) => log::info!("Component {:?} installed successfully", id),
        Err(e) => log::error!("Failed to install component {:?}: {}", id, e),
    }

    result
}

/// Reinstall a component by id, dispatching to the appropriate sub-module.
pub async fn reinstall_component(
    client: &Client,
    id: ComponentId,
    app_handle: Option<&AppHandle>,
) -> Result<String> {
    match id {
        ComponentId::Python => python::reinstall_component(client, app_handle).await,
        ComponentId::Nodejs => nodejs::reinstall_nodejs(client, app_handle).await,
        ComponentId::UV => uv::reinstall_uv(client, app_handle).await,
    }
}

/// Build the PATH environment variable for an instance.
///
/// Order: venv_bin → uv dir → nodejs shim dir → system PATH
pub fn build_instance_path(venv_python: &Path) -> Result<OsString> {
    let venv_bin = venv_python
        .parent()
        .ok_or_else(|| AppError::io("Invalid venv python path"))?;

    let mut paths = vec![venv_bin.to_path_buf()];

    // Make uv/uvx directly invokable by child processes.
    let uv_dir = get_component_dir("uv");
    if uv::is_uv_installed() {
        paths.push(uv_dir);
    }

    // If Node.js component is installed, add the shim directory only.
    // The shims themselves prepend the real node/npm bin dirs internally.
    let nodejs_dir = get_component_dir("nodejs");
    let node_exe = get_node_exe_path(&nodejs_dir);
    if node_exe.exists() {
        paths.push(get_nodejs_shim_dir());
    }

    // Append system PATH (filtering duplicates)
    if let Some(existing) = env::var_os("PATH") {
        let extra: Vec<_> = env::split_paths(&existing)
            .filter(|p| !paths.contains(p))
            .collect();
        paths.extend(extra);
    }

    env::join_paths(paths)
        .map_err(|e| AppError::io(format!("Failed to build instance PATH: {}", e)))
}
