mod common;
mod node_shim;
mod nodejs;
mod python;
mod registry;
mod types;
mod uv;

use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::path::Path;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};

use reqwest::Client;
use tauri::AppHandle;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

use crate::error::{AppError, Result};
use crate::utils::paths::{get_component_dir, get_node_exe_path, get_nodejs_shim_dir};

pub use node_shim::generate_shims;
pub use nodejs::build_nodejs_env_vars;
pub use python::{create_venv, pip_install_requirements};
pub use types::{ComponentId, ComponentsSnapshot};
pub use uv::{clear_uv_cache, is_uv_installed, uv_sync};

use types::ComponentStatus;

/// Serializes install/reinstall/uninstall commands per component, so
/// overlapping commands cannot race on the same runtime directories and
/// archive files.
static COMPONENT_CMD_LOCKS: OnceLock<StdMutex<HashMap<ComponentId, Arc<AsyncMutex<()>>>>> =
    OnceLock::new();

fn component_locks() -> &'static StdMutex<HashMap<ComponentId, Arc<AsyncMutex<()>>>> {
    COMPONENT_CMD_LOCKS.get_or_init(|| StdMutex::new(HashMap::new()))
}

async fn acquire_component_lock(id: ComponentId) -> OwnedMutexGuard<()> {
    let mutex = {
        let mut locks = component_locks()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        Arc::clone(locks.entry(id).or_default())
    };
    mutex.lock_owned().await
}

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
    let _guard = acquire_component_lock(id).await;
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

/// Uninstall a component by id, dispatching to the appropriate sub-module.
pub async fn uninstall_component(id: ComponentId) -> Result<String> {
    let _guard = acquire_component_lock(id).await;
    log::info!("Uninstalling component {:?}", id);
    let result = match id {
        ComponentId::Python => python::uninstall_component(),
        ComponentId::Nodejs => nodejs::uninstall_nodejs(),
        ComponentId::UV => uv::uninstall_uv(),
    };

    match &result {
        Ok(msg) => log::info!("Component {:?} uninstalled: {}", id, msg),
        Err(e) => log::error!("Failed to uninstall component {:?}: {}", id, e),
    }

    result
}

/// Reinstall a component by id, dispatching to the appropriate sub-module.
pub async fn reinstall_component(
    client: &Client,
    id: ComponentId,
    app_handle: Option<&AppHandle>,
) -> Result<String> {
    let _guard = acquire_component_lock(id).await;
    match id {
        ComponentId::Python => python::reinstall_component(client, app_handle).await,
        ComponentId::Nodejs => nodejs::reinstall_nodejs(client, app_handle).await,
        ComponentId::UV => uv::reinstall_uv(client, app_handle).await,
    }
}

/// Build the PATH environment variable for an instance.
///
/// Order: venv_bin → uv dir → nodejs shim dir → (optional) system PATH
pub fn build_instance_path(venv_python: &Path, ignore_external_path: bool) -> Result<OsString> {
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

    if !ignore_external_path {
        // Append system PATH (filtering duplicates)
        if let Some(existing) = env::var_os("PATH") {
            let extra: Vec<_> = env::split_paths(&existing)
                .filter(|p| !paths.contains(p))
                .collect();
            paths.extend(extra);
        }
    }

    env::join_paths(paths)
        .map_err(|e| AppError::io(format!("Failed to build instance PATH: {}", e)))
}
