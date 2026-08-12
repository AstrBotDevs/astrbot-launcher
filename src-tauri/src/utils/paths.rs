//! Centralized path utilities for the application.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::error::{AppError, Result};
use crate::utils::sync::{read_lock_recover, write_lock_recover};

/// Directory (under the OS config directory) used for launcher-internal files
/// that must live outside the user-selectable data directory, such as the
/// data directory override itself.
const LAUNCHER_CONFIG_SUBDIR: &str = "com.github.raven95676.astrbot-launcher";

/// File storing the user-configured data directory override.
const DATA_DIR_OVERRIDE_FILE: &str = ".data-dir.json";

/// Cached resolved data directory. `None` means "not resolved yet".
static DATA_DIR_CACHE: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Get the launcher config directory (`<config_dir>/com.github.raven95676.astrbot-launcher`).
pub(crate) fn launcher_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join(LAUNCHER_CONFIG_SUBDIR))
}

fn data_dir_override_path() -> Option<PathBuf> {
    launcher_config_dir().map(|dir| dir.join(DATA_DIR_OVERRIDE_FILE))
}

/// The default data directory (~/.astrbot_launcher).
#[allow(clippy::expect_used)]
pub(crate) fn default_data_dir() -> PathBuf {
    let home = dirs::home_dir().expect("Cannot find home directory");
    home.join(".astrbot_launcher")
}

/// Validate a user-provided data directory path.
pub(crate) fn validate_data_dir(dir: &Path) -> Result<()> {
    if dir.as_os_str().is_empty() {
        return Err(AppError::other("数据目录不能为空"));
    }
    if !dir.is_absolute() {
        return Err(AppError::other("数据目录必须是绝对路径"));
    }
    if dir.is_file() {
        return Err(AppError::other(
            "目标路径已存在且是一个文件，请选择一个文件夹",
        ));
    }
    Ok(())
}

#[derive(serde::Deserialize)]
struct DataDirOverrideFile {
    data_dir: String,
}

/// Load the data directory override from disk, if any.
fn load_data_dir_override() -> Option<PathBuf> {
    let path = data_dir_override_path()?;
    let content = fs::read_to_string(&path).ok()?;
    let parsed: DataDirOverrideFile = match serde_json::from_str(&content) {
        Ok(value) => value,
        Err(error) => {
            log::warn!(
                "Data dir override file is corrupted, ignoring it: {}",
                error
            );
            return None;
        }
    };
    let dir = PathBuf::from(parsed.data_dir.trim());
    if validate_data_dir(&dir).is_err() {
        log::warn!(
            "Stored data dir override is invalid, ignoring it: {}",
            dir.display()
        );
        return None;
    }
    Some(dir)
}

/// Get the root data directory for the application.
///
/// Uses the user-configured override if present, otherwise falls back to
/// the default (~/.astrbot_launcher).
pub(crate) fn get_data_dir() -> PathBuf {
    let cached = read_lock_recover(&DATA_DIR_CACHE, "DATA_DIR_CACHE").clone();
    if let Some(dir) = cached {
        return dir;
    }

    let dir = load_data_dir_override().unwrap_or_else(default_data_dir);
    let mut guard = write_lock_recover(&DATA_DIR_CACHE, "DATA_DIR_CACHE");
    if guard.is_none() {
        *guard = Some(dir.clone());
    }
    dir
}

/// Persist a new data directory override, or remove the override to fall back
/// to the default when `new_dir` is `None`.
///
/// Takes effect for all path helpers immediately and for the rest of the
/// application after a restart. Does not migrate any data.
pub(crate) fn set_data_dir_override(new_dir: Option<&Path>) -> Result<()> {
    let config_dir = launcher_config_dir()
        .ok_or_else(|| AppError::other("无法确定应用配置目录，请检查操作系统环境后重试"))?;
    let override_path = config_dir.join(DATA_DIR_OVERRIDE_FILE);

    if let Some(dir) = new_dir {
        validate_data_dir(dir)?;
        fs::create_dir_all(&config_dir).map_err(|e| AppError::io(e.to_string()))?;
        let payload = serde_json::json!({ "data_dir": dir.to_string_lossy() });
        fs::write(&override_path, payload.to_string()).map_err(|e| AppError::io(e.to_string()))?;

        let mut guard = write_lock_recover(&DATA_DIR_CACHE, "DATA_DIR_CACHE");
        *guard = Some(dir.to_path_buf());
    } else {
        match fs::remove_file(&override_path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(AppError::io(e.to_string())),
        }
        let mut guard = write_lock_recover(&DATA_DIR_CACHE, "DATA_DIR_CACHE");
        *guard = Some(default_data_dir());
    }
    Ok(())
}

/// Get the path to the unified application data database.
pub(crate) fn data_db_path() -> PathBuf {
    get_data_dir().join("data.redb")
}

/// Get the path to the legacy config TOML file (migration only).
pub(crate) fn config_path() -> PathBuf {
    get_data_dir().join("config.toml")
}

/// Get the path to the legacy manifest TOML file (migration only).
pub(crate) fn manifest_path() -> PathBuf {
    get_data_dir().join("manifest.toml")
}

/// Get the path to the releases cache file.
pub(crate) fn version_list_cache_path() -> PathBuf {
    get_data_dir().join("version_list.json")
}

/// Ensure all required data directories exist.
pub(crate) fn ensure_data_dirs() -> Result<()> {
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
pub(crate) fn get_instance_dir(instance_id: &str) -> PathBuf {
    get_data_dir().join("instances").join(instance_id)
}

/// Get the core directory for an instance.
pub(crate) fn get_instance_core_dir(instance_id: &str) -> PathBuf {
    get_instance_dir(instance_id).join("core")
}

/// Get the virtual environment directory for an instance.
pub(crate) fn get_instance_venv_dir(instance_id: &str) -> PathBuf {
    get_instance_dir(instance_id).join("venv")
}

/// Get the versions directory.
pub(crate) fn get_versions_dir() -> PathBuf {
    get_data_dir().join("versions")
}

/// Get the zip file path for a specific version (e.g., versions/v4.14.8.zip).
pub(crate) fn get_version_zip_path(version: &str) -> PathBuf {
    get_versions_dir().join(format!("{}.zip", version))
}

/// Get the backups directory.
pub(crate) fn get_backups_dir() -> PathBuf {
    get_data_dir().join("backups")
}

/// Get the root components directory.
pub(crate) fn get_components_dir() -> PathBuf {
    get_data_dir().join("components")
}

/// Get a specific component's directory.
pub(crate) fn get_component_dir(dir_name: &str) -> PathBuf {
    get_components_dir().join(dir_name)
}

/// Get Python runtime directory under the unified python component.
pub(crate) fn get_python_runtime_dir(runtime: &str) -> PathBuf {
    get_component_dir("python").join(runtime)
}

fn join_segments(base: &Path, segments: &[&str]) -> PathBuf {
    let mut path = base.to_path_buf();
    path.extend(segments);
    path
}

#[cfg(target_os = "windows")]
fn platform_join(base: &Path, windows_segments: &[&str], _unix_segments: &[&str]) -> PathBuf {
    join_segments(base, windows_segments)
}

#[cfg(not(target_os = "windows"))]
fn platform_join(base: &Path, _windows_segments: &[&str], unix_segments: &[&str]) -> PathBuf {
    join_segments(base, unix_segments)
}

/// Get the path to the Python executable for a standalone Python directory.
pub(crate) fn get_python_exe_path(python_dir: &Path) -> PathBuf {
    platform_join(python_dir, &["python.exe"], &["bin", "python3"])
}

/// Get the path to the Node.js executable for a standalone Node directory.
pub(crate) fn get_node_exe_path(node_dir: &Path) -> PathBuf {
    platform_join(node_dir, &["node.exe"], &["bin", "node"])
}

/// Get the path to the npm executable for a standalone Node directory.
pub(crate) fn get_npm_exe_path(node_dir: &Path) -> PathBuf {
    platform_join(node_dir, &["npm.cmd"], &["bin", "npm"])
}

/// Get the path to the npx executable for a standalone Node directory.
pub(crate) fn get_npx_exe_path(node_dir: &Path) -> PathBuf {
    platform_join(node_dir, &["npx.cmd"], &["bin", "npx"])
}

/// Get the bin directory for a standalone Node directory.
pub(crate) fn get_node_bin_dir(node_dir: &Path) -> PathBuf {
    platform_join(node_dir, &[], &["bin"])
}

/// Get the npm global install prefix directory (component-level, shared by all instances).
pub(crate) fn get_nodejs_npm_prefix() -> PathBuf {
    get_component_dir("nodejs")
}

/// Get the npm cache directory (component-level, shared by all instances).
pub(crate) fn get_nodejs_npm_cache() -> PathBuf {
    get_component_dir("nodejs").join(".npm_cache")
}

/// Get the shim scripts directory for Node.js.
pub(crate) fn get_nodejs_shim_dir() -> PathBuf {
    get_component_dir("nodejs").join("shims")
}

/// Get the bin directory under an npm prefix (where globally installed binaries go).
pub(crate) fn get_npm_prefix_bin_dir(npm_prefix: &Path) -> PathBuf {
    platform_join(npm_prefix, &[], &["bin"])
}

/// Get the node_modules directory under an npm prefix.
pub(crate) fn get_npm_prefix_modules_dir(npm_prefix: &Path) -> PathBuf {
    platform_join(npm_prefix, &["node_modules"], &["lib", "node_modules"])
}

/// Get the Python executable path within a virtual environment.
pub(crate) fn get_venv_python(venv_dir: &Path) -> PathBuf {
    platform_join(venv_dir, &["Scripts", "python.exe"], &["bin", "python"])
}

/// Get uv executable path within uv component directory.
pub(crate) fn get_uv_exe_path(uv_dir: &Path) -> PathBuf {
    platform_join(uv_dir, &["uv.exe"], &["uv"])
}

/// Get uvx executable path within uv component directory.
pub(crate) fn get_uvx_exe_path(uv_dir: &Path) -> PathBuf {
    platform_join(uv_dir, &["uvx.exe"], &["uvx"])
}

/// Get uv cache directory (component-level, shared by all instances).
pub(crate) fn get_uv_cache_dir() -> PathBuf {
    get_component_dir("uv").join("cache")
}
