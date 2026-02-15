//! Centralized path utilities for the application.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{AppError, Result};

/// Get the root data directory for the application (~/.astrbot_launcher).
#[allow(clippy::expect_used)]
pub fn get_data_dir() -> PathBuf {
    let home = dirs::home_dir().expect("Cannot find home directory");
    home.join(".astrbot_launcher")
}

/// Get the path to the config file.
pub fn config_path() -> PathBuf {
    get_data_dir().join("config.toml")
}

/// Ensure all required data directories exist.
pub fn ensure_data_dirs() -> Result<()> {
    let base = get_data_dir();
    fs::create_dir_all(&base).map_err(|e| AppError::io(e.to_string()))?;

    let dirs = [
        base.join("components"),
        base.join("versions"),
        base.join("instances"),
        base.join("backups"),
    ];
    for dir in &dirs {
        fs::create_dir_all(dir).map_err(|e| AppError::io(e.to_string()))?;
    }
    Ok(())
}

/// Get the root directory for an instance.
pub fn get_instance_dir(instance_id: &str) -> PathBuf {
    get_data_dir().join("instances").join(instance_id)
}

/// Get the deployment marker file path for an instance.
pub fn get_instance_deploy_marker(instance_id: &str) -> PathBuf {
    get_instance_dir(instance_id).join(".deployed")
}

/// Get the core directory for an instance.
pub fn get_instance_core_dir(instance_id: &str) -> PathBuf {
    get_instance_dir(instance_id).join("core")
}

/// Get the virtual environment directory for an instance.
pub fn get_instance_venv_dir(instance_id: &str) -> PathBuf {
    get_instance_dir(instance_id).join("venv")
}

/// Check if an instance is fully deployed
pub fn is_instance_deployed(instance_id: &str) -> bool {
    let marker = get_instance_deploy_marker(instance_id);
    if !marker.exists() {
        return false;
    }

    let core_dir = get_instance_core_dir(instance_id);
    let venv_dir = get_instance_venv_dir(instance_id);
    let venv_python = get_venv_python(&venv_dir);
    core_dir.join("main.py").exists() && venv_python.exists()
}

/// Get the versions directory.
pub fn get_versions_dir() -> PathBuf {
    get_data_dir().join("versions")
}

/// Get the zip file path for a specific version (e.g., versions/v4.14.8.zip).
pub fn get_version_zip_path(version: &str) -> PathBuf {
    get_versions_dir().join(format!("{}.zip", version))
}

/// Get the backups directory.
pub fn get_backups_dir() -> PathBuf {
    get_data_dir().join("backups")
}

/// Get the root components directory.
pub fn get_components_dir() -> PathBuf {
    get_data_dir().join("components")
}

/// Get a specific component's directory.
pub fn get_component_dir(dir_name: &str) -> PathBuf {
    get_components_dir().join(dir_name)
}

/// Get the path to the Python executable for a standalone Python directory.
pub fn get_python_exe_path(python_dir: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        python_dir.join("python.exe")
    }

    #[cfg(not(target_os = "windows"))]
    {
        python_dir.join("bin").join("python3")
    }
}

/// Get the path to the Node.js executable for a standalone Node directory.
pub fn get_node_exe_path(node_dir: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        node_dir.join("node.exe")
    }

    #[cfg(not(target_os = "windows"))]
    {
        node_dir.join("bin").join("node")
    }
}

/// Get the path to the npm executable for a standalone Node directory.
pub fn get_npm_exe_path(node_dir: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        node_dir.join("npm.cmd")
    }

    #[cfg(not(target_os = "windows"))]
    {
        node_dir.join("bin").join("npm")
    }
}

/// Get the path to the npx executable for a standalone Node directory.
pub fn get_npx_exe_path(node_dir: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        node_dir.join("npx.cmd")
    }

    #[cfg(not(target_os = "windows"))]
    {
        node_dir.join("bin").join("npx")
    }
}

/// Get the bin directory for a standalone Node directory.
pub fn get_node_bin_dir(node_dir: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        node_dir.to_path_buf()
    }

    #[cfg(not(target_os = "windows"))]
    {
        node_dir.join("bin")
    }
}

/// Get the npm global install prefix directory (component-level, shared by all instances).
pub fn get_nodejs_npm_prefix() -> PathBuf {
    get_component_dir("nodejs")
}

/// Get the npm cache directory (component-level, shared by all instances).
pub fn get_nodejs_npm_cache() -> PathBuf {
    get_component_dir("nodejs").join(".npm_cache")
}

/// Get the shim scripts directory for Node.js.
pub fn get_nodejs_shim_dir() -> PathBuf {
    get_component_dir("nodejs").join("shims")
}

/// Get the bin directory under an npm prefix (where globally installed binaries go).
pub fn get_npm_prefix_bin_dir(npm_prefix: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        npm_prefix.to_path_buf()
    }

    #[cfg(not(target_os = "windows"))]
    {
        npm_prefix.join("bin")
    }
}

/// Get the node_modules directory under an npm prefix.
pub fn get_npm_prefix_modules_dir(npm_prefix: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        npm_prefix.join("node_modules")
    }

    #[cfg(not(target_os = "windows"))]
    {
        npm_prefix.join("lib").join("node_modules")
    }
}

/// Get the Python executable path within a virtual environment.
pub fn get_venv_python(venv_dir: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        venv_dir.join("Scripts").join("python.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        venv_dir.join("bin").join("python")
    }
}
