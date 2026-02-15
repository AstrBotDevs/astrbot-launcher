mod migration;
mod node_shim;
mod nodejs;
mod python;
mod types;

use std::env;
use std::ffi::{OsStr, OsString};
use std::path::Path;

use reqwest::Client;
use tauri::AppHandle;

use crate::config::load_config;
use crate::error::{AppError, Result};
use crate::paths::{
    get_component_dir, get_node_exe_path, get_nodejs_npm_cache, get_nodejs_npm_prefix,
    get_nodejs_shim_dir, get_npm_prefix_modules_dir,
};

pub use migration::migrate_legacy_python_dirs;
pub use node_shim::generate_shims;
pub use python::get_python_for_version;
pub use types::{ComponentId, ComponentsSnapshot};

use types::ComponentStatus;

/// Build a snapshot of all component statuses.
pub fn build_components_snapshot() -> ComponentsSnapshot {
    let components = ComponentId::all()
        .iter()
        .map(|&id| {
            let installed = match id {
                ComponentId::Python312 | ComponentId::Python310 => {
                    python::is_component_installed(id)
                }
                ComponentId::NodejsLts => nodejs::is_nodejs_installed(),
            };
            ComponentStatus {
                id: id.dir_name().to_string(),
                installed,
                display_name: id.display_name().to_string(),
                description: format!("{} 运行时", id.display_name()),
            }
        })
        .collect();

    ComponentsSnapshot { components }
}

/// Install a component by id, dispatching to the appropriate sub-module.
pub async fn install_component(client: &Client, id: ComponentId, app_handle: Option<&AppHandle>) -> Result<String> {
    match id {
        ComponentId::Python312 | ComponentId::Python310 => {
            python::install_component(client, id, app_handle).await
        }
        ComponentId::NodejsLts => nodejs::install_nodejs(client, app_handle).await,
    }
}

/// Reinstall a component by id, dispatching to the appropriate sub-module.
pub async fn reinstall_component(client: &Client, id: ComponentId, app_handle: Option<&AppHandle>) -> Result<String> {
    match id {
        ComponentId::Python312 | ComponentId::Python310 => {
            python::reinstall_component(client, id, app_handle).await
        }
        ComponentId::NodejsLts => nodejs::reinstall_nodejs(client, app_handle).await,
    }
}

/// Build the PATH environment variable for an instance.
///
/// Order: venv_bin → nodejs shim dir → system PATH
pub fn build_instance_path(venv_python: &Path) -> Result<OsString> {
    let venv_bin = venv_python
        .parent()
        .ok_or_else(|| AppError::io("Invalid venv python path"))?;

    let mut paths = vec![venv_bin.to_path_buf()];

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

/// Build Node.js environment variables (component-level isolation).
///
/// Returns a list of (key, value) pairs. Each npm config variable is emitted in
/// both uppercase and lowercase forms for maximum compatibility.
/// Returns an empty vec if Node.js is not installed.
pub fn build_nodejs_env_vars() -> Vec<(OsString, OsString)> {
    let nodejs_dir = get_component_dir("nodejs");
    if !get_node_exe_path(&nodejs_dir).exists() {
        return Vec::new();
    }

    let npm_prefix = get_nodejs_npm_prefix();
    let npm_cache = get_nodejs_npm_cache();

    // Ensure directories exist
    std::fs::create_dir_all(&npm_prefix).ok();
    std::fs::create_dir_all(&npm_cache).ok();

    let modules_dir = get_npm_prefix_modules_dir(&npm_prefix);

    let mut vars: Vec<(OsString, OsString)> = Vec::new();

    // Helper: push both UPPER and lower case versions
    let mut push_both = |upper: &str, lower: &str, val: &OsStr| {
        vars.push((upper.into(), val.to_os_string()));
        vars.push((lower.into(), val.to_os_string()));
    };

    push_both("NODE_PATH", "node_path", modules_dir.as_os_str());
    push_both(
        "NPM_CONFIG_PREFIX",
        "npm_config_prefix",
        npm_prefix.as_os_str(),
    );
    push_both(
        "NPM_CONFIG_CACHE",
        "npm_config_cache",
        npm_cache.as_os_str(),
    );

    // Point globalconfig / userconfig to files inside our prefix so npm never
    // reads the system-wide or user-level npmrc that could override isolation.
    let global_npmrc = npm_prefix.join("etc").join("npmrc");
    push_both(
        "NPM_CONFIG_GLOBALCONFIG",
        "npm_config_globalconfig",
        global_npmrc.as_os_str(),
    );
    // npm-globalconfig is an alias recognised by some npm versions
    push_both(
        "NPM_CONFIG_NPM_GLOBALCONFIG",
        "npm_config_npm_globalconfig",
        global_npmrc.as_os_str(),
    );
    let user_npmrc = npm_prefix.join(".npmrc");
    push_both(
        "NPM_CONFIG_USERCONFIG",
        "npm_config_userconfig",
        user_npmrc.as_os_str(),
    );

    if let Ok(config) = load_config() {
        if !config.npm_registry.is_empty() {
            push_both(
                "NPM_CONFIG_REGISTRY",
                "npm_config_registry",
                OsStr::new(&config.npm_registry),
            );
        }
    }

    vars
}
