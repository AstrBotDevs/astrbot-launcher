use std::fs::{self, File};
use std::io::Read as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use tar::Archive;

use crate::archive::{
    append_dir_tree_to_zip, extract_tar_gz_mapped, extract_zip_mapped, parse_entry_rel_path,
};
use crate::config::{load_config, with_config_mut, BackupInfo, BackupMetadata, InstanceConfig};
use crate::error::{AppError, Result};
use crate::paths::{get_backups_dir, get_instance_core_dir, get_instance_dir};
use crate::validation::{resolve_backup_path, validate_instance_id};

/// Check if a backup path is in tar.gz format.
fn is_tar_gz(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.ends_with(".tar.gz"))
        .unwrap_or(false)
}

/// Common backup creation logic.
fn create_backup_archive(
    instance: &InstanceConfig,
    instance_id: &str,
    auto_generated: bool,
) -> Result<String> {
    let backups_dir = get_backups_dir();
    fs::create_dir_all(&backups_dir)
        .map_err(|e| AppError::backup(format!("Failed to create backups dir: {}", e)))?;

    let core_dir = get_instance_core_dir(instance_id);
    let data_dir = core_dir.join("data");

    // Generate backup filename
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let filename = if auto_generated {
        format!("{}-{}-auto.zip", instance_id, timestamp)
    } else {
        format!("{}-{}.zip", instance_id, timestamp)
    };
    let backup_path = backups_dir.join(&filename);

    // Create metadata
    let metadata = BackupMetadata {
        created_at: chrono::Utc::now().to_rfc3339(),
        instance_name: instance.name.clone(),
        instance_id: instance_id.to_string(),
        version: instance.version.clone(),
        includes_venv: false,
        includes_data: true,
        arch_target: String::new(),
        auto_generated,
    };

    let file = File::create(&backup_path)
        .map_err(|e| AppError::backup(format!("Failed to create backup archive: {}", e)))?;
    let mut writer = zip::ZipWriter::new(file);

    // Write metadata as backup.toml
    let metadata_toml = toml::to_string_pretty(&metadata)
        .map_err(|e| AppError::backup(format!("Failed to serialize metadata: {}", e)))?;
    let options = zip::write::SimpleFileOptions::default();
    writer
        .start_file("backup.toml", options)
        .map_err(|e| AppError::backup(format!("Failed to add metadata: {}", e)))?;
    writer
        .write_all(metadata_toml.as_bytes())
        .map_err(|e| AppError::backup(format!("Failed to write metadata: {}", e)))?;

    // Add data directory
    if data_dir.exists() {
        append_dir_tree_to_zip(&mut writer, &data_dir, "data", options)
            .map_err(|e| AppError::backup(format!("Failed to add data dir: {}", e)))?;
    }

    writer
        .finish()
        .map_err(|e| AppError::backup(format!("Failed to finalize backup archive: {}", e)))?;

    Ok(backup_path
        .to_str()
        .ok_or_else(|| AppError::io("backup path is not valid UTF-8"))?
        .to_string())
}

/// Create a backup of an instance.
///
/// When `auto_generated` is `true` the backup is tagged in its metadata and
/// hidden from the user-facing backup list.
pub fn create_backup(instance_id: &str, auto_generated: bool) -> Result<String> {
    let config = load_config()?;
    let instance = config
        .instances
        .get(instance_id)
        .ok_or_else(|| AppError::instance_not_found(instance_id))?;

    let data_dir = get_instance_core_dir(instance_id).join("data");
    if !data_dir.exists() {
        return Err(AppError::backup("No data directory to back up"));
    }

    create_backup_archive(instance, instance_id, auto_generated)
}

/// List all backups
pub fn list_backups() -> Result<Vec<BackupInfo>> {
    let backups_dir = get_backups_dir();
    if !backups_dir.exists() {
        return Ok(Vec::new());
    }

    let mut backups = Vec::new();

    for entry in fs::read_dir(&backups_dir)
        .map_err(|e| AppError::backup(format!("Failed to read backups dir: {}", e)))?
    {
        let entry = entry.map_err(|e| AppError::backup(e.to_string()))?;
        let path = entry.path();

        let fname = match path.file_name().and_then(|n| n.to_str()) {
            Some(s) => s.to_string(),
            None => {
                log::warn!("Skipping backup with non-UTF-8 filename: {:?}", path);
                continue;
            }
        };

        // Accept both .tar.gz and .zip
        if !(fname.ends_with(".tar.gz") || fname.ends_with(".zip")) {
            continue;
        }

        let path_str = match path.to_str() {
            Some(s) => s.to_string(),
            None => {
                log::warn!("Skipping backup with non-UTF-8 path: {:?}", path);
                continue;
            }
        };

        match read_backup_metadata(&path) {
            Ok(metadata) => {
                // Skip auto-generated backups
                if metadata.auto_generated {
                    continue;
                }
                backups.push(BackupInfo {
                    filename: fname,
                    path: path_str,
                    metadata,
                    corrupted: false,
                    parse_error: None,
                });
            }
            Err(err) => {
                log::warn!("Backup metadata parse failed for {:?}: {}", path, err);
                backups.push(BackupInfo {
                    filename: fname.clone(),
                    path: path_str,
                    metadata: BackupMetadata {
                        created_at: String::new(),
                        instance_name: "(损坏备份)".to_string(),
                        instance_id: String::new(),
                        version: String::new(),
                        includes_venv: false,
                        includes_data: false,
                        arch_target: String::new(),
                        auto_generated: false,
                    },
                    corrupted: true,
                    parse_error: Some(err.to_string()),
                });
            }
        }
    }

    // Sort by created_at descending
    backups.sort_by(|a, b| b.metadata.created_at.cmp(&a.metadata.created_at));

    Ok(backups)
}

fn read_backup_metadata(backup_path: &Path) -> Result<BackupMetadata> {
    if is_tar_gz(backup_path) {
        read_backup_metadata_tar_gz(backup_path)
    } else {
        read_backup_metadata_zip(backup_path)
    }
}

fn read_backup_metadata_tar_gz(backup_path: &Path) -> Result<BackupMetadata> {
    let file = File::open(backup_path)
        .map_err(|e| AppError::backup(format!("Failed to open backup: {}", e)))?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);

    for entry in archive
        .entries()
        .map_err(|e| AppError::backup(e.to_string()))?
    {
        let mut entry = entry.map_err(|e| AppError::backup(e.to_string()))?;
        let path = entry.path().map_err(|e| AppError::backup(e.to_string()))?;

        if path.to_str() == Some("backup.toml") {
            let mut content = String::new();
            entry
                .read_to_string(&mut content)
                .map_err(|e| AppError::backup(e.to_string()))?;
            return toml::from_str(&content)
                .map_err(|e| AppError::backup(format!("Failed to parse metadata: {}", e)));
        }
    }

    Err(AppError::backup("backup.toml not found in tar.gz backup"))
}

fn read_backup_metadata_zip(backup_path: &Path) -> Result<BackupMetadata> {
    let file = File::open(backup_path)
        .map_err(|e| AppError::backup(format!("Failed to open backup: {}", e)))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| AppError::backup(format!("Failed to read zip: {}", e)))?;

    let mut entry = archive
        .by_name("backup.toml")
        .map_err(|e| AppError::backup(format!("backup.toml not found: {}", e)))?;

    let mut content = String::new();
    entry
        .read_to_string(&mut content)
        .map_err(|e| AppError::backup(e.to_string()))?;

    toml::from_str(&content)
        .map_err(|e| AppError::backup(format!("Failed to parse metadata: {}", e)))
}

/// Restore a backup to its original instance
pub fn restore_backup(backup_path: &str) -> Result<()> {
    let backup_path = resolve_backup_path(backup_path, true)?;

    // Read metadata
    let metadata = read_backup_metadata(&backup_path)?;

    // Check if version is installed
    let config = load_config()?;
    if !config
        .installed_versions
        .iter()
        .any(|v| v.version == metadata.version)
    {
        return Err(AppError::version_not_found(&metadata.version));
    }

    // Validate original instance still exists
    let instance_id = &metadata.instance_id;
    if !config.instances.contains_key(instance_id) {
        return Err(AppError::instance_not_found(instance_id));
    }

    let instance_dir = get_instance_dir(instance_id);
    let core_dir = get_instance_core_dir(instance_id);

    // Extract backup to existing instance
    extract_backup_to_instance(&backup_path, &instance_dir, &core_dir)?;

    // Update instance version if different
    with_config_mut(|config| {
        if let Some(instance) = config.instances.get_mut(instance_id) {
            instance.version = metadata.version.clone();
        }
        Ok(())
    })?;

    Ok(())
}

/// Route an archive entry to the correct destination directory.
fn route_backup_entry(relative: &Path, instance_dir: &Path, core_dir: &Path) -> Option<PathBuf> {
    // Skip backup.toml
    if relative == Path::new("backup.toml") {
        return None;
    }

    // Skip venv entries from old backups
    if relative.starts_with("venv") {
        return None;
    }

    // Route data/ entries to core_dir, everything else to instance_dir
    if relative.starts_with("data") {
        Some(core_dir.join(relative))
    } else {
        Some(instance_dir.join(relative))
    }
}

/// Extract backup archive to instance directories.
fn extract_backup_to_instance(
    backup_path: &Path,
    instance_dir: &Path,
    core_dir: &Path,
) -> Result<()> {
    let routing = |raw_path: &str| -> Option<PathBuf> {
        let relative = parse_entry_rel_path(raw_path)?;
        route_backup_entry(&relative, instance_dir, core_dir)
    };

    if is_tar_gz(backup_path) {
        extract_tar_gz_mapped(backup_path, instance_dir, routing)
    } else {
        extract_zip_mapped(backup_path, instance_dir, routing)
    }
    .map_err(|e| AppError::backup(format!("Failed to extract backup: {}", e)))
}

/// Delete a backup
pub fn delete_backup(backup_path: &str) -> Result<()> {
    let path = resolve_backup_path(backup_path, false)?;
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|e| AppError::backup(format!("Failed to delete backup: {}", e)))?;
    }
    Ok(())
}

/// Restore only data from a backup to an existing instance.
pub fn restore_data_to_instance(backup_path: &str, instance_id: &str) -> Result<()> {
    validate_instance_id(instance_id)?;

    let backup_path = resolve_backup_path(backup_path, true)?;
    let core_dir = get_instance_core_dir(instance_id);

    let routing = |raw_path: &str| -> Option<PathBuf> {
        let relative = parse_entry_rel_path(raw_path)?;
        if !relative.starts_with("data") {
            return None;
        }

        Some(core_dir.join(&relative))
    };

    if is_tar_gz(&backup_path) {
        extract_tar_gz_mapped(&backup_path, &core_dir, routing)
    } else {
        extract_zip_mapped(&backup_path, &core_dir, routing)
    }
    .map_err(|e| AppError::backup(format!("Failed to restore backup data: {}", e)))
}
