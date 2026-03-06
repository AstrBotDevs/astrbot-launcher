use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use crate::error::{AppError, Result};
use crate::utils::archive_path::parse_entry_rel_path;

use super::extract::write_entry;
use super::links::{create_queued_symlinks, queue_symlink, QueuedSymlink};
use super::path::{detect_common_top_dir, resolve_within_dir, strip_common_top_dir};

/// Extract zip entries using a caller-provided destination resolver.
///
/// Returning `None` from `destination_for` skips the entry.
pub(crate) fn extract_zip_mapped<F>(
    archive_path: &Path,
    dest_dir: &Path,
    mut destination_for: F,
) -> Result<()>
where
    F: FnMut(&str) -> Option<PathBuf>,
{
    fs::create_dir_all(dest_dir).map_err(|e| AppError::io(e.to_string()))?;
    let file = fs::File::open(archive_path).map_err(|e| AppError::io(e.to_string()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| AppError::io(e.to_string()))?;
    let mut pending_symlinks: Vec<QueuedSymlink> = Vec::new();

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| AppError::io(e.to_string()))?;

        let raw_name = entry.name().to_string();

        if parse_entry_rel_path(&raw_name).is_none() {
            return Err(AppError::io(format!(
                "archive contains unsafe zip path: {raw_name:?}"
            )));
        }

        let Some(out_path) = destination_for(&raw_name) else {
            continue;
        };
        let resolved_out_path = resolve_within_dir(dest_dir, &out_path)?;

        if entry.is_symlink() {
            let mut target = String::new();
            entry
                .read_to_string(&mut target)
                .map_err(|e| AppError::io(e.to_string()))?;
            let pending = queue_symlink(&resolved_out_path, Path::new(&target), dest_dir)?;
            pending_symlinks.push(pending);
        } else {
            let is_dir = entry.is_dir();
            let unix_mode = entry.unix_mode();
            let declared_size = if is_dir { None } else { Some(entry.size()) };
            write_entry(
                &resolved_out_path,
                is_dir,
                &mut entry,
                unix_mode,
                declared_size,
            )?;
        }
    }

    create_queued_symlinks(pending_symlinks)?;

    Ok(())
}

/// Extract zip archive to dest_dir, stripping the top-level directory from the archive.
pub(crate) fn extract_zip_flat(archive_path: &Path, dest_dir: &Path) -> Result<()> {
    let top_dir = {
        let file = fs::File::open(archive_path).map_err(|e| AppError::io(e.to_string()))?;
        let archive = zip::ZipArchive::new(file).map_err(|e| AppError::io(e.to_string()))?;
        detect_common_top_dir(archive.file_names())
    };

    extract_zip_mapped(archive_path, dest_dir, |raw_path| {
        let relative = parse_entry_rel_path(raw_path)?;
        let stripped = strip_common_top_dir(&relative, top_dir.as_deref())?;
        Some(dest_dir.join(stripped))
    })
}
