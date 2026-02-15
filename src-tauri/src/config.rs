use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};
use crate::paths::{config_path, ensure_data_dirs};

static CONFIG_LOCK: Mutex<()> = Mutex::new(());
static CONFIG_CACHE: OnceLock<RwLock<Arc<AppConfig>>> = OnceLock::new();

fn lock_mutex_recover<'a, T>(lock: &'a Mutex<T>, name: &str) -> MutexGuard<'a, T> {
    match lock.lock() {
        Ok(guard) => guard,
        Err(e) => {
            log::warn!("{} mutex poisoned, recovering inner state", name);
            e.into_inner()
        }
    }
}

fn read_lock_recover<'a, T>(lock: &'a RwLock<T>, name: &str) -> RwLockReadGuard<'a, T> {
    match lock.read() {
        Ok(guard) => guard,
        Err(e) => {
            log::warn!("{} read lock poisoned, recovering inner state", name);
            e.into_inner()
        }
    }
}

fn write_lock_recover<'a, T>(lock: &'a RwLock<T>, name: &str) -> RwLockWriteGuard<'a, T> {
    match lock.write() {
        Ok(guard) => guard,
        Err(e) => {
            log::warn!("{} write lock poisoned, recovering inner state", name);
            e.into_inner()
        }
    }
}

fn load_config_from_disk() -> Result<AppConfig> {
    let path = config_path();
    if !path.exists() {
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
    fs::write(config_path(), content).map_err(|e| AppError::config(e.to_string()))
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

    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub instances: HashMap<String, InstanceConfig>,
    #[serde(default)]
    pub installed_versions: Vec<InstalledVersion>,
    #[serde(default)]
    pub github_proxy: String,
    #[serde(default)]
    pub pypi_mirror: String,
    #[serde(default)]
    pub nodejs_mirror: String,
    #[serde(default)]
    pub npm_registry: String,
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
    #[serde(default = "default_true")]
    pub check_instance_update: bool,
    #[serde(default)]
    pub persist_instance_state: bool,
    #[serde(default)]
    pub tracked_instances_snapshot: Vec<String>,
}

fn default_true() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            instances: HashMap::new(),
            installed_versions: Vec::new(),
            github_proxy: String::new(),
            pypi_mirror: String::new(),
            nodejs_mirror: String::new(),
            npm_registry: String::new(),
            close_to_tray: true,
            check_instance_update: true,
            persist_instance_state: false,
            tracked_instances_snapshot: Vec::new(),
        }
    }
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
