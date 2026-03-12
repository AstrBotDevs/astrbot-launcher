use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};
use crate::utils::paths::{config_path, ensure_data_dirs, manifest_path};
use crate::utils::sync::{lock_mutex_recover, read_lock_recover, write_lock_recover};

static CONFIG_LOCK: Mutex<()> = Mutex::new(());
static CONFIG_CACHE: OnceLock<RwLock<Arc<AppConfig>>> = OnceLock::new();
static MANIFEST_LOCK: Mutex<()> = Mutex::new(());
static MANIFEST_CACHE: OnceLock<RwLock<Arc<AppManifest>>> = OnceLock::new();

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub mainland_acceleration: bool,
    #[serde(default)]
    pub github_proxy: String,
    #[serde(default)]
    pub proxy_url: String,
    #[serde(default)]
    pub proxy_port: String,
    #[serde(default)]
    pub proxy_username: String,
    #[serde(default)]
    pub proxy_password: String,
    #[serde(default)]
    pub pypi_mirror: String,
    #[serde(default)]
    pub nodejs_mirror: String,
    #[serde(default)]
    pub npm_registry: String,
    #[serde(default)]
    pub use_uv_for_deps: bool,
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
    #[serde(default = "default_true")]
    pub check_instance_update: bool,
    #[serde(default)]
    pub persist_instance_state: bool,
    #[serde(default)]
    pub ignore_external_path: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            mainland_acceleration: false,
            github_proxy: String::new(),
            proxy_url: String::new(),
            proxy_port: String::new(),
            proxy_username: String::new(),
            proxy_password: String::new(),
            pypi_mirror: String::new(),
            nodejs_mirror: String::new(),
            npm_registry: String::new(),
            use_uv_for_deps: false,
            close_to_tray: true,
            check_instance_update: true,
            persist_instance_state: false,
            ignore_external_path: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppManifest {
    #[serde(default)]
    pub instances: HashMap<String, InstanceConfig>,
    #[serde(default)]
    pub installed_versions: Vec<InstalledVersion>,
    #[serde(default)]
    pub tracked_instances_snapshot: Vec<String>,
}

fn load_config_from_disk() -> Result<AppConfig> {
    let path = config_path();
    if !path.exists() {
        log::debug!("Config file not found, creating default");
        let config = AppConfig::default();
        save_config_to_disk(&config)?;
        return Ok(config);
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::config(e.to_string()))?;
    toml::from_str(&content).map_err(|e| AppError::config(e.to_string()))
}

fn save_config_to_disk(config: &AppConfig) -> Result<()> {
    ensure_data_dirs()?;
    let content = toml::to_string_pretty(config).map_err(|e| AppError::config(e.to_string()))?;
    fs::write(config_path(), content).map_err(|e| {
        log::error!("Failed to write config to disk: {}", e);
        AppError::config(e.to_string())
    })
}

fn load_manifest_from_disk() -> Result<AppManifest> {
    let path = manifest_path();
    if !path.exists() {
        log::debug!("Manifest file not found, creating default");
        let manifest = AppManifest::default();
        save_manifest_to_disk(&manifest)?;
        return Ok(manifest);
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::config(e.to_string()))?;
    toml::from_str(&content).map_err(|e| AppError::config(e.to_string()))
}

fn save_manifest_to_disk(manifest: &AppManifest) -> Result<()> {
    ensure_data_dirs()?;
    let content = toml::to_string_pretty(manifest).map_err(|e| AppError::config(e.to_string()))?;
    fs::write(manifest_path(), content).map_err(|e| {
        log::error!("Failed to write manifest to disk: {}", e);
        AppError::config(e.to_string())
    })
}

fn get_config_cache() -> Result<&'static RwLock<Arc<AppConfig>>> {
    if let Some(cache) = CONFIG_CACHE.get() {
        return Ok(cache);
    }

    let config = load_config_from_disk()?;
    let _ = CONFIG_CACHE.set(RwLock::new(Arc::new(config)));

    CONFIG_CACHE
        .get()
        .ok_or_else(|| AppError::config("CONFIG_CACHE not initialized"))
}

fn get_manifest_cache() -> Result<&'static RwLock<Arc<AppManifest>>> {
    if let Some(cache) = MANIFEST_CACHE.get() {
        return Ok(cache);
    }

    let manifest = load_manifest_from_disk()?;
    let _ = MANIFEST_CACHE.set(RwLock::new(Arc::new(manifest)));

    MANIFEST_CACHE
        .get()
        .ok_or_else(|| AppError::config("MANIFEST_CACHE not initialized"))
}

/// Execute a read-modify-write operation on the config file while holding a lock.
/// This prevents concurrent modifications from causing data loss.
pub fn with_config_mut<F, T>(f: F) -> Result<T>
where
    F: FnOnce(&mut AppConfig) -> Result<T>,
{
    let _guard = lock_mutex_recover(&CONFIG_LOCK, "CONFIG_LOCK");
    let cache = get_config_cache()?;

    let current = {
        let config = read_lock_recover(cache, "CONFIG_CACHE");
        Arc::clone(&config)
    };

    let mut updated = (*current).clone();
    let result = f(&mut updated)?;
    save_config_to_disk(&updated)?;

    *write_lock_recover(cache, "CONFIG_CACHE") = Arc::new(updated);
    log::debug!("Config saved");

    Ok(result)
}

/// Execute a read-modify-write operation on the manifest file while holding a lock.
/// This prevents concurrent modifications from causing data loss.
pub fn with_manifest_mut<F, T>(f: F) -> Result<T>
where
    F: FnOnce(&mut AppManifest) -> Result<T>,
{
    let _guard = lock_mutex_recover(&MANIFEST_LOCK, "MANIFEST_LOCK");
    let cache = get_manifest_cache()?;

    let current = {
        let manifest = read_lock_recover(cache, "MANIFEST_CACHE");
        Arc::clone(&manifest)
    };

    let mut updated = (*current).clone();
    let result = f(&mut updated)?;
    save_manifest_to_disk(&updated)?;

    *write_lock_recover(cache, "MANIFEST_CACHE") = Arc::new(updated);
    log::debug!("Manifest saved");

    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceConfig {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstalledVersion {
    pub version: String,
    pub zip_path: String,
}

/// Backup metadata stored in backup.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupMetadata {
    pub created_at: String,
    pub instance_name: String,
    pub instance_id: String,
    pub version: String,
    #[serde(default)]
    pub includes_venv: bool,
    #[serde(default = "default_true")]
    pub includes_data: bool,
    #[serde(default)]
    pub arch_target: String,
    #[serde(default)]
    pub auto_generated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    pub filename: String,
    pub path: String,
    pub metadata: BackupMetadata,
    #[serde(default)]
    pub corrupted: bool,
    #[serde(default)]
    pub parse_error: Option<String>,
}

pub fn load_config() -> Result<Arc<AppConfig>> {
    let cache = get_config_cache()?;
    let config = read_lock_recover(cache, "CONFIG_CACHE");
    Ok(Arc::clone(&config))
}

pub fn reload_config() -> Result<Arc<AppConfig>> {
    let _guard = lock_mutex_recover(&CONFIG_LOCK, "CONFIG_LOCK");
    let cache = get_config_cache()?;
    let fresh = Arc::new(load_config_from_disk()?);
    *write_lock_recover(cache, "CONFIG_CACHE") = Arc::clone(&fresh);
    Ok(fresh)
}

pub fn load_manifest() -> Result<Arc<AppManifest>> {
    let cache = get_manifest_cache()?;
    let manifest = read_lock_recover(cache, "MANIFEST_CACHE");
    Ok(Arc::clone(&manifest))
}

pub fn reload_manifest() -> Result<Arc<AppManifest>> {
    let _guard = lock_mutex_recover(&MANIFEST_LOCK, "MANIFEST_LOCK");
    let cache = get_manifest_cache()?;
    let fresh = Arc::new(load_manifest_from_disk()?);
    *write_lock_recover(cache, "MANIFEST_CACHE") = Arc::clone(&fresh);
    Ok(fresh)
}
