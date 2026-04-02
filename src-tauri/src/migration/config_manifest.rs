use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::{
    has_config_record, has_manifest_record, with_config_mut, with_manifest_mut, AppConfig,
    AppManifest, InstalledVersion, InstanceConfig,
};
use crate::utils::paths::{config_path, manifest_path};

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
            lock_check_extension_whitelist: false,
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

fn read_toml_file(path: &Path, file_name: &str) -> Option<String> {
    if !path.exists() {
        return None;
    }

    match fs::read_to_string(path) {
        Ok(content) => Some(content),
        Err(error) => {
            log::warn!("Migration: failed to read {}: {}", file_name, error);
            None
        }
    }
}

/// Merge manifest data from `source` into `target`.
/// Existing entries in `target` keep priority on conflicts.
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

fn load_legacy_state(
    config_toml: Option<String>,
    manifest_toml: Option<String>,
) -> (Option<AppConfig>, Option<AppManifest>) {
    let parsed_config =
        config_toml
            .as_deref()
            .and_then(|content| match toml::from_str::<AppConfig>(content) {
                Ok(config) => Some(config),
                Err(error) => {
                    log::warn!(
                        "Migration: failed to parse config.toml as AppConfig: {}",
                        error
                    );
                    toml::from_str::<LegacyAppConfig>(content)
                        .map(LegacyAppConfig::into_config)
                        .map_err(|legacy_error| {
                            log::warn!(
                                "Migration: failed to parse config.toml as LegacyAppConfig: {}",
                                legacy_error
                            );
                            legacy_error
                        })
                        .ok()
                }
            });

    let manifest_from_file = manifest_toml.as_deref().and_then(|content| {
        toml::from_str::<AppManifest>(content)
            .map_err(|error| {
                log::warn!("Migration: failed to parse manifest.toml: {}", error);
                error
            })
            .ok()
    });

    let manifest_from_config = config_toml
        .as_deref()
        .filter(|content| has_manifest_fields(content))
        .and_then(|content| {
            toml::from_str::<LegacyAppConfig>(content)
                .map(LegacyAppConfig::into_manifest)
                .map_err(|error| {
                    log::warn!(
                        "Migration: failed to parse legacy manifest fields from config.toml: {}",
                        error
                    );
                    error
                })
                .ok()
        });

    let merged_manifest = match (manifest_from_file, manifest_from_config) {
        (Some(mut file_manifest), Some(config_manifest)) => {
            merge_manifest(&mut file_manifest, &config_manifest);
            Some(file_manifest)
        }
        (Some(file_manifest), None) => Some(file_manifest),
        (None, Some(config_manifest)) => Some(config_manifest),
        (None, None) => None,
    };

    (parsed_config, merged_manifest)
}

pub fn migrate_config_manifest_if_needed() {
    let config_missing = match has_config_record() {
        Ok(has) => !has,
        Err(error) => {
            log::warn!(
                "Migration: failed to inspect redb config record before migration: {}",
                error
            );
            return;
        }
    };
    let manifest_missing = match has_manifest_record() {
        Ok(has) => !has,
        Err(error) => {
            log::warn!(
                "Migration: failed to inspect redb manifest record before migration: {}",
                error
            );
            return;
        }
    };

    if !config_missing && !manifest_missing {
        return;
    }

    let config_toml = read_toml_file(&config_path(), "config.toml");
    let manifest_toml = read_toml_file(&manifest_path(), "manifest.toml");
    let (config_from_legacy, manifest_from_legacy) = load_legacy_state(config_toml, manifest_toml);

    if config_missing {
        let imported_config = config_from_legacy.unwrap_or_default();
        if let Err(error) = with_config_mut(move |config| {
            *config = imported_config;
            Ok(())
        }) {
            log::warn!("Migration: failed to import config into redb: {}", error);
            return;
        }
    }

    if manifest_missing {
        let imported_manifest = manifest_from_legacy.unwrap_or_default();

        if let Err(error) = with_manifest_mut(move |manifest| {
            *manifest = imported_manifest;
            Ok(())
        }) {
            log::warn!("Migration: failed to import manifest into redb: {}", error);
            return;
        }
    }

    log::info!("Migrated legacy config/manifest TOML data into data.redb");
}
