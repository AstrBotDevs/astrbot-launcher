//! Instance data and cache cleanup utilities.

use std::path::Path;

use walkdir::WalkDir;

use crate::error::{AppError, Result};
use crate::paths::{get_instance_core_dir, get_instance_venv_dir};
use crate::validation::validate_instance_id;

/// Clear instance data directory.
pub fn clear_instance_data(instance_id: &str) -> Result<()> {
    validate_instance_id(instance_id)?;

    let core_dir = get_instance_core_dir(instance_id);
    let data_dir = core_dir.join("data");

    if data_dir.exists() {
        std::fs::remove_dir_all(&data_dir)
            .map_err(|e| AppError::io(format!("Failed to clear data: {}", e)))?;
    }
    Ok(())
}

/// Clear instance venv.
pub fn clear_instance_venv(instance_id: &str) -> Result<()> {
    validate_instance_id(instance_id)?;

    let venv_dir = get_instance_venv_dir(instance_id);
    if venv_dir.exists() {
        std::fs::remove_dir_all(&venv_dir)
            .map_err(|e| AppError::io(format!("Failed to clear venv: {}", e)))?;
    }
    Ok(())
}

/// Clear Python bytecode cache (__pycache__).
pub fn clear_pycache(instance_id: &str) -> Result<()> {
    validate_instance_id(instance_id)?;

    let core_dir = get_instance_core_dir(instance_id);
    let venv_dir = get_instance_venv_dir(instance_id);

    if core_dir.exists() {
        clear_pycache_recursive(&core_dir)?;
    }
    if venv_dir.exists() {
        clear_pycache_recursive(&venv_dir)?;
    }

    Ok(())
}

fn clear_pycache_recursive(dir: &Path) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    let mut iter = WalkDir::new(dir).into_iter();

    while let Some(entry) = iter.next() {
        let entry = entry.map_err(|e| AppError::io(e.to_string()))?;
        let path = entry.path();

        if entry.file_type().is_dir() && entry.file_name() == "__pycache__" {
            iter.skip_current_dir();
            if let Err(e) = std::fs::remove_dir_all(path) {
                log::warn!("Failed to remove __pycache__ {:?}: {}", path, e);
            }
            continue;
        }

        if entry.file_type().is_file() && path.extension().map(|e| e == "pyc").unwrap_or(false) {
            if let Err(e) = std::fs::remove_file(path) {
                log::warn!("Failed to remove .pyc file {:?}: {}", path, e);
            }
        }
    }

    Ok(())
}
