use std::ffi::OsStr;
use std::fs::{self, File};
use std::io;
use std::io::Read as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use tar::Archive;
use walkdir::WalkDir;

use crate::archive::{extract_tar_gz_mapped, extract_zip_mapped};
use crate::config::{load_manifest, with_manifest_mut, BackupInfo, BackupMetadata, InstanceConfig};
use crate::error::{AppError, Result};
use crate::utils::archive_path::parse_entry_rel_path;
use crate::utils::lock_check::{collect_files_for_lock_check, ensure_target_not_locked};
use crate::utils::paths::{get_backups_dir, get_instance_core_dir, get_instance_dir};
use crate::utils::validation::validate_instance_id;
use crate::validation::resolve_backup_path;

/// Check if a backup path is in tar.gz format.
fn is_tar_gz(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.ends_with(".tar.gz"))
        .unwrap_or(false)
}

fn path_contains_pycache(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == OsStr::new("__pycache__"))
}

fn append_data_dir_to_zip_excluding_pycache<W: io::Write + io::Seek>(
    writer: &mut zip::ZipWriter<W>,
    data_dir: &Path,
    options: zip::write::SimpleFileOptions,
) -> Result<()> {
    let mut iter = WalkDir::new(data_dir).into_iter();
    while let Some(entry) = iter.next() {
        let entry = entry.map_err(|e| AppError::io(e.to_string()))?;
        let path = entry.path();

        if entry.file_type().is_dir() && entry.file_name() == OsStr::new("__pycache__") {
            iter.skip_current_dir();
            continue;
        }

        let relative = path
            .strip_prefix(data_dir)
            .map_err(|e| AppError::io(e.to_string()))?;
        // Use forward slashes in archive paths per ZIP spec, regardless of OS.
        let rel_parts: Vec<_> = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect();
        let archive_path = format!("data/{}", rel_parts.join("/"));

        if entry.file_type().is_dir() {
            if path != data_dir {
                writer
                    .add_directory(&archive_path, options)
                    .map_err(|e| AppError::io(e.to_string()))?;
            }
            continue;
        }

        if entry.file_type().is_file() {
            writer
                .start_file(&archive_path, options)
                .map_err(|e| AppError::io(e.to_string()))?;
            let mut f = fs::File::open(path).map_err(|e| AppError::io(e.to_string()))?;
            io::copy(&mut f, writer).map_err(|e| AppError::io(e.to_string()))?;
        }
    }

    Ok(())
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
        append_data_dir_to_zip_excluding_pycache(&mut writer, &data_dir, options)
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
/// When `auto_generated` is `true`, the backup is tagged in metadata so UI can
/// show it as an auto-generated backup.
pub fn create_backup(instance_id: &str, auto_generated: bool) -> Result<String> {
    log::info!("Creating backup for instance {}", instance_id);
    let manifest = load_manifest()?;
    let instance = manifest
        .instances
        .get(instance_id)
        .ok_or_else(|| AppError::instance_not_found(instance_id))?;

    let data_dir = get_instance_core_dir(instance_id).join("data");
    if !data_dir.exists() {
        return Err(AppError::backup("No data directory to back up"));
    }
    let data_files = collect_files_for_lock_check(&data_dir)?;
    ensure_target_not_locked(&data_files)?;

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

/// Find the latest auto-generated backup for the specified instance, if any.
///
/// This is used to detect unfinished upgrade/downgrade recovery flows.
pub fn find_pending_auto_backup(instance_id: &str) -> Result<Option<BackupInfo>> {
    validate_instance_id(instance_id)?;

    let backups_dir = get_backups_dir();
    if !backups_dir.exists() {
        return Ok(None);
    }

    let filename_prefix = format!("{instance_id}-");
    let mut latest: Option<(BackupInfo, std::time::SystemTime)> = None;

    for entry in fs::read_dir(&backups_dir)
        .map_err(|e| AppError::backup(format!("Failed to read backups dir: {}", e)))?
    {
        let entry = entry.map_err(|e| AppError::backup(e.to_string()))?;
        let path = entry.path();

        let Some(filename) = path.file_name().and_then(|n| n.to_str()) else {
            log::warn!("Skipping backup with non-UTF-8 filename: {:?}", path);
            continue;
        };
        let is_supported = filename.ends_with(".zip") || filename.ends_with(".tar.gz");
        let is_auto_candidate =
            filename.starts_with(&filename_prefix) && filename.contains("-auto");
        if !is_supported || !is_auto_candidate {
            continue;
        }

        let Some(mtime) = fs::metadata(&path)
            .ok()
            .and_then(|metadata| metadata.modified().ok())
        else {
            continue;
        };

        let metadata = match read_backup_metadata(&path) {
            Ok(metadata) => metadata,
            Err(err) => {
                log::warn!("Backup metadata parse failed for {:?}: {}", path, err);
                continue;
            }
        };
        if !metadata.auto_generated || metadata.instance_id != instance_id {
            continue;
        }

        let Some(path_str) = path.to_str() else {
            log::warn!("Skipping backup with non-UTF-8 path: {:?}", path);
            continue;
        };

        let info = BackupInfo {
            filename: filename.to_string(),
            path: path_str.to_string(),
            metadata,
            corrupted: false,
            parse_error: None,
        };

        match &latest {
            None => latest = Some((info, mtime)),
            Some((current, current_mtime)) => {
                let is_newer = mtime
                    .cmp(current_mtime)
                    .then_with(|| info.metadata.created_at.cmp(&current.metadata.created_at))
                    .is_gt();
                if is_newer {
                    latest = Some((info, mtime));
                }
            }
        }
    }

    Ok(latest.map(|(backup, _)| backup))
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

fn cleanup_auto_backup(backup_path: &Path, operation_name: &str) {
    if let Err(e) = fs::remove_file(backup_path) {
        log::warn!(
            "{} succeeded, but failed to delete auto-generated backup {:?}: {}",
            operation_name,
            backup_path,
            e
        );
    } else {
        log::info!(
            "Deleted auto-generated backup after successful {}: {:?}",
            operation_name,
            backup_path
        );
    }
}

pub fn resolve_restore_backup_input(backup_path: &str) -> Result<(PathBuf, BackupMetadata)> {
    log::info!("Restoring backup from {:?}", backup_path);
    let backup_path = resolve_backup_path(backup_path, true)?;

    // Read metadata
    let metadata = read_backup_metadata(&backup_path)?;
    Ok((backup_path, metadata))
}

pub fn restore_backup_with_input(backup_path: PathBuf, metadata: BackupMetadata) -> Result<()> {
    // Check if version is installed
    let manifest = load_manifest()?;
    if !manifest
        .installed_versions
        .iter()
        .any(|v| v.version == metadata.version)
    {
        return Err(AppError::version_not_found(&metadata.version));
    }

    // Validate original instance still exists
    let instance_id = &metadata.instance_id;
    if !manifest.instances.contains_key(instance_id) {
        return Err(AppError::instance_not_found(instance_id));
    }

    let instance_dir = get_instance_dir(instance_id);
    let core_dir = get_instance_core_dir(instance_id);
    let data_dir = core_dir.join("data");
    let data_files = collect_files_for_lock_check(&data_dir)?;
    ensure_target_not_locked(&data_files)?;

    // Extract backup to existing instance
    extract_backup_to_instance(&backup_path, &instance_dir, &core_dir)?;

    // Update instance version if different
    with_manifest_mut(|manifest| {
        if let Some(instance) = manifest.instances.get_mut(instance_id) {
            instance.version = metadata.version.clone();
        }
        Ok(())
    })?;

    if metadata.auto_generated {
        cleanup_auto_backup(&backup_path, "restore");
    }

    Ok(())
}

/// Route an archive entry to the correct destination directory.
fn route_backup_entry(relative: &Path, core_dir: &Path) -> Option<PathBuf> {
    // Skip backup.toml
    if relative == Path::new("backup.toml") {
        return None;
    }

    if relative.starts_with("data") {
        if path_contains_pycache(relative) {
            return None;
        }
        return Some(core_dir.join(relative));
    }
    if relative.starts_with("venv") {
        return None;
    }

    log::warn!(
        "Ignoring unsupported backup entry during restore: {}",
        relative.display()
    );
    None
}

/// Extract backup archive to instance directories.
fn extract_backup_to_instance(
    backup_path: &Path,
    instance_dir: &Path,
    core_dir: &Path,
) -> Result<()> {
    let routing = |raw_path: &str| -> Option<PathBuf> {
        let relative = parse_entry_rel_path(raw_path)?;
        route_backup_entry(&relative, core_dir)
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
    log::info!("Deleting backup {:?}", backup_path);
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
    let metadata = read_backup_metadata(&backup_path)?;
    let core_dir = get_instance_core_dir(instance_id);
    let data_dir = core_dir.join("data");
    let data_files = collect_files_for_lock_check(&data_dir)?;
    ensure_target_not_locked(&data_files)?;

    let routing = |raw_path: &str| -> Option<PathBuf> {
        let relative = parse_entry_rel_path(raw_path)?;
        if !relative.starts_with("data") {
            return None;
        }
        if path_contains_pycache(&relative) {
            return None;
        }

        Some(core_dir.join(&relative))
    };

    if is_tar_gz(&backup_path) {
        extract_tar_gz_mapped(&backup_path, &core_dir, routing)
    } else {
        extract_zip_mapped(&backup_path, &core_dir, routing)
    }
    .map_err(|e| AppError::backup(format!("Failed to restore backup data: {}", e)))?;

    if metadata.auto_generated {
        cleanup_auto_backup(&backup_path, "data restore");
    }

    Ok(())
}
