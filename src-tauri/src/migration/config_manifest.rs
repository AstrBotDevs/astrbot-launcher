use std::collections::HashMap;
use std::fs;

use serde::{Deserialize, Serialize};

use crate::config::{AppConfig, AppManifest, InstalledVersion, InstanceConfig};
use crate::error::{AppError, Result};
use crate::utils::paths::{config_path, ensure_data_dirs, manifest_path};

const MANIFEST_FIELDS: [&str; 3] = [
    "instances",
    "installed_versions",
    "tracked_instances_snapshot",
];

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyAppConfig {
    #[serde(default)]
    mainland_acceleration: bool,
    #[serde(default)]
    instances: HashMap<String, InstanceConfig>,
    #[serde(default)]
    installed_versions: Vec<InstalledVersion>,
    #[serde(default)]
    github_proxy: String,
    #[serde(default)]
    proxy_url: String,
    #[serde(default)]
    proxy_port: String,
    #[serde(default)]
    proxy_username: String,
    #[serde(default)]
    proxy_password: String,
    #[serde(default)]
    pypi_mirror: String,
    #[serde(default)]
    nodejs_mirror: String,
    #[serde(default)]
    npm_registry: String,
    #[serde(default)]
    use_uv_for_deps: bool,
    #[serde(default = "default_true")]
    close_to_tray: bool,
    #[serde(default = "default_true")]
    check_instance_update: bool,
    #[serde(default)]
    persist_instance_state: bool,
    #[serde(default)]
    ignore_external_path: bool,
    #[serde(default)]
    tracked_instances_snapshot: Vec<String>,
}

impl LegacyAppConfig {
    fn into_config(self) -> AppConfig {
        AppConfig {
            mainland_acceleration: self.mainland_acceleration,
            github_proxy: self.github_proxy,
            proxy_url: self.proxy_url,
            proxy_port: self.proxy_port,
            proxy_username: self.proxy_username,
            proxy_password: self.proxy_password,
            pypi_mirror: self.pypi_mirror,
            nodejs_mirror: self.nodejs_mirror,
            npm_registry: self.npm_registry,
            use_uv_for_deps: self.use_uv_for_deps,
            close_to_tray: self.close_to_tray,
            check_instance_update: self.check_instance_update,
            persist_instance_state: self.persist_instance_state,
            ignore_external_path: self.ignore_external_path,
        }
    }

    fn into_manifest(self) -> AppManifest {
        AppManifest {
            instances: self.instances,
            installed_versions: self.installed_versions,
            tracked_instances_snapshot: self.tracked_instances_snapshot,
        }
    }
}

fn has_manifest_fields(content: &str) -> bool {
    toml::from_str::<toml::Value>(content)
        .ok()
        .and_then(|value| value.as_table().cloned())
        .map(|table| {
            MANIFEST_FIELDS
                .iter()
                .any(|field| table.contains_key(*field))
        })
        .unwrap_or(false)
}

fn save_config_to_disk(config: &AppConfig) -> Result<()> {
    ensure_data_dirs()?;
    let content = toml::to_string_pretty(config).map_err(|e| AppError::config(e.to_string()))?;
    fs::write(config_path(), content).map_err(|e| AppError::config(e.to_string()))
}

fn save_manifest_to_disk(manifest: &AppManifest) -> Result<()> {
    ensure_data_dirs()?;
    let content = toml::to_string_pretty(manifest).map_err(|e| AppError::config(e.to_string()))?;
    fs::write(manifest_path(), content).map_err(|e| AppError::config(e.to_string()))
}

fn read_manifest_from_disk() -> Option<AppManifest> {
    let content = fs::read_to_string(manifest_path()).ok()?;
    toml::from_str::<AppManifest>(&content).ok()
}

/// Merge legacy manifest data into an existing manifest.
/// Existing entries in `target` take priority over `source` on conflicts.
fn merge_manifest(target: &mut AppManifest, source: &AppManifest) {
    for (id, instance) in &source.instances {
        target
            .instances
            .entry(id.clone())
            .or_insert_with(|| instance.clone());
    }

    for version in &source.installed_versions {
        if !target
            .installed_versions
            .iter()
            .any(|v| v.version == version.version)
        {
            target.installed_versions.push(version.clone());
        }
    }

    for id in &source.tracked_instances_snapshot {
        if !target.tracked_instances_snapshot.contains(id) {
            target.tracked_instances_snapshot.push(id.clone());
        }
    }
}

pub fn migrate_config_manifest_if_needed() {
    let config_file = config_path();
    if !config_file.exists() {
        return;
    }

    let content = match fs::read_to_string(&config_file) {
        Ok(content) => content,
        Err(e) => {
            log::warn!(
                "Migration: failed to read config.toml during config/manifest migration: {}",
                e
            );
            return;
        }
    };

    if !has_manifest_fields(&content) {
        // config.toml is already clean, nothing to migrate.
        return;
    }

    let legacy = match toml::from_str::<LegacyAppConfig>(&content) {
        Ok(legacy) => legacy,
        Err(e) => {
            log::warn!(
                "Migration: failed to parse config.toml during config/manifest migration: {}",
                e
            );
            return;
        }
    };

    let legacy_manifest = legacy.clone().into_manifest();
    let manifest_file = manifest_path();

    if manifest_file.exists() {
        // Manifest exists — merge legacy data into it.
        match read_manifest_from_disk() {
            Some(mut existing) => {
                merge_manifest(&mut existing, &legacy_manifest);
                if let Err(e) = save_manifest_to_disk(&existing) {
                    log::warn!("Migration: failed to write merged manifest.toml: {}", e);
                    return;
                }
            }
            None => {
                // Manifest file exists but cannot be parsed — overwrite with legacy data.
                if let Err(e) = save_manifest_to_disk(&legacy_manifest) {
                    log::warn!("Migration: failed to write manifest.toml: {}", e);
                    return;
                }
            }
        }
    } else {
        // No manifest file — create from legacy.
        if let Err(e) = save_manifest_to_disk(&legacy_manifest) {
            log::warn!(
                "Migration: failed to write manifest.toml during config/manifest migration: {}",
                e
            );
            return;
        }
    }

    // Clean manifest fields from config.toml.
    let clean_config = legacy.into_config();
    if let Err(e) = save_config_to_disk(&clean_config) {
        log::warn!(
            "Migration: failed to rewrite config.toml during config/manifest migration: {}",
            e
        );
        return;
    }

    log::info!("Migrated manifest fields from config.toml to manifest.toml");
}
