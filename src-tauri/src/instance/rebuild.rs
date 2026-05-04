//! Rebuild instance and version manifest entries by scanning the data directory.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::config::{with_manifest_mut, InstalledVersion, InstanceConfig};
use crate::error::{AppError, Result};
use crate::utils::paths::{get_data_dir, get_versions_dir};
use crate::utils::validation::validate_instance_id;

#[derive(Debug, Clone, serde::Serialize)]
pub struct RebuildInstanceManifestResult {
    pub instances: usize,
    pub versions: usize,
}

pub fn rebuild_instance_manifest_from_disk() -> Result<RebuildInstanceManifestResult> {
    let installed_versions = scan_installed_versions()?;
    let instances = scan_instances()?;
    let instance_count = instances.len();
    let version_count = installed_versions.len();

    with_manifest_mut(move |manifest| {
        manifest.installed_versions = installed_versions;
        manifest.instances = instances;
        let rebuilt_ids: Vec<String> = manifest.instances.keys().cloned().collect();
        manifest
            .tracked_instances_snapshot
            .retain(|id| rebuilt_ids.contains(id));
        Ok(())
    })?;

    Ok(RebuildInstanceManifestResult {
        instances: instance_count,
        versions: version_count,
    })
}

fn scan_installed_versions() -> Result<Vec<InstalledVersion>> {
    let versions_dir = get_versions_dir();
    if !versions_dir.exists() {
        return Ok(Vec::new());
    }

    let mut versions = Vec::new();
    for entry in fs::read_dir(&versions_dir)
        .map_err(|e| AppError::io(format!("Failed to read versions dir: {}", e)))?
    {
        let entry = entry.map_err(|e| AppError::io(e.to_string()))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
            log::warn!(
                "Skipping version archive with non-UTF-8 filename: {:?}",
                path
            );
            continue;
        };
        let Some(version) = filename.strip_suffix(".zip") else {
            continue;
        };
        let Some(zip_path) = path.to_str() else {
            log::warn!("Skipping version archive with non-UTF-8 path: {:?}", path);
            continue;
        };

        versions.push(InstalledVersion {
            version: version.to_string(),
            zip_path: zip_path.to_string(),
        });
    }

    Ok(versions)
}

fn scan_instances() -> Result<HashMap<String, InstanceConfig>> {
    let instances_dir = get_data_dir().join("instances");
    if !instances_dir.exists() {
        return Ok(HashMap::new());
    }

    let mut instances = HashMap::new();

    for entry in fs::read_dir(&instances_dir)
        .map_err(|e| AppError::io(format!("Failed to read instances dir: {}", e)))?
    {
        let entry = entry.map_err(|e| AppError::io(e.to_string()))?;
        if !entry
            .file_type()
            .map_err(|e| AppError::io(e.to_string()))?
            .is_dir()
        {
            continue;
        }

        let Some(instance_id) = entry.file_name().to_str().map(str::to_string) else {
            log::warn!(
                "Skipping instance directory with non-UTF-8 name: {:?}",
                entry.path()
            );
            continue;
        };
        if let Err(error) = validate_instance_id(&instance_id) {
            log::warn!(
                "Skipping invalid instance directory {}: {}",
                instance_id,
                error
            );
            continue;
        }

        let instance_dir = entry.path();
        if !looks_like_instance_dir(&instance_dir) {
            log::warn!(
                "Skipping instance directory without recognizable content: {:?}",
                instance_dir
            );
            continue;
        }

        let Some(version) = read_pyproject_version(&instance_dir) else {
            log::warn!(
                "Skipping instance {} because core/pyproject.toml has no project.version",
                instance_id
            );
            continue;
        };

        let instance = InstanceConfig {
            name: default_instance_name(&instance_id),
            version,
            port: 0,
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        instances.insert(instance_id, instance);
    }

    Ok(instances)
}

fn looks_like_instance_dir(instance_dir: &Path) -> bool {
    let core_dir = instance_dir.join("core");
    instance_dir.join("venv").exists()
        || core_dir.join("main.py").exists()
        || core_dir.join("data").exists()
        || core_dir.join("pyproject.toml").exists()
        || core_dir.join("requirements.txt").exists()
}

fn read_pyproject_version(instance_dir: &Path) -> Option<String> {
    let pyproject_path = instance_dir.join("core").join("pyproject.toml");
    let content = fs::read_to_string(&pyproject_path).ok()?;
    let value = match toml::from_str::<toml::Value>(&content) {
        Ok(value) => value,
        Err(error) => {
            log::warn!("Failed to parse {:?}: {}", pyproject_path, error);
            return None;
        }
    };
    let version = value
        .get("project")
        .and_then(|project| project.get("version"))
        .and_then(toml::Value::as_str)?
        .trim();

    if version.is_empty() {
        return None;
    }
    if version.starts_with('v') {
        Some(version.to_string())
    } else {
        Some(format!("v{}", version))
    }
}

fn default_instance_name(instance_id: &str) -> String {
    let short_id: String = instance_id.chars().take(8).collect();
    format!("Rev {}", short_id)
}
