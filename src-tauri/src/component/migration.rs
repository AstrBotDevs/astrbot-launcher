use std::fs;
use std::path::{Path, PathBuf};

use crate::paths::{get_component_dir, get_data_dir};

/// Migrate legacy `python/` and `compat_python/` directories to the new
/// `components/python312` and `components/python310` layout.
///
/// Migration errors are logged but never crash the app.
pub fn migrate_legacy_python_dirs() {
    let data_dir = get_data_dir();

    migrate_dir(
        &data_dir.join("python"),
        &get_component_dir("python312"),
        "python/ -> components/python312",
    );

    migrate_dir(
        &data_dir.join("compat_python"),
        &get_component_dir("python310"),
        "compat_python/ -> components/python310",
    );

    if let Err(e) = migrate_instance_pyvenv_cfgs(&data_dir) {
        log::warn!(
            "Migration: failed to update instance pyvenv.cfg files: {}",
            e
        );
    }
}

fn migrate_dir(src: &Path, dst: &Path, label: &str) {
    if !src.exists() {
        return;
    }
    if dst.exists() {
        // Destination already exists — remove the legacy source to clean up.
        log::info!("Migration {}: destination already exists, removing legacy dir", label);
        if let Err(e) = fs::remove_dir_all(src) {
            log::warn!("Migration {}: failed to remove legacy dir: {}", label, e);
        }
        return;
    }
    
    // Ensure parent of dst exists
    if let Some(parent) = dst.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            log::warn!("Migration {}: failed to create parent dir: {}", label, e);
            return;
        }
    }
    
    log::info!("Migration {}: renaming", label);
    if let Err(e) = fs::rename(src, dst) {
        log::warn!("Migration {}: rename failed: {}", label, e);
    }
}

fn migrate_instance_pyvenv_cfgs(data_dir: &Path) -> Result<(), String> {
    let instances_dir = data_dir.join("instances");
    if !instances_dir.exists() {
        return Ok(());
    }
    
    let to_absolute = |p: PathBuf| -> Option<String> {
        if let Ok(canonical) = p.canonicalize() {
            canonical.to_str().map(|s| s.to_string())
        } else {
            std::env::current_dir()
                .ok()
                .and_then(|cwd| cwd.join(&p).to_str().map(|s| s.to_string()))
                .or_else(|| p.to_str().map(|s| s.to_string()))
        }
    };
    
    let legacy_python = to_absolute(data_dir.join("python"));
    let legacy_compat = to_absolute(data_dir.join("compat_python"));
    let target_python312 = to_absolute(get_component_dir("python312"));
    let target_python310 = to_absolute(get_component_dir("python310"));
    
    let mut replacements = Vec::new();
    if let (Some(legacy), Some(target)) = (legacy_python, target_python312) {
        replacements.push((legacy, target));
    }
    if let (Some(legacy), Some(target)) = (legacy_compat, target_python310) {
        replacements.push((legacy, target));
    }
    
    if replacements.is_empty() {
        return Ok(());
    }
    
    for entry in fs::read_dir(&instances_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let pyvenv = entry.path().join("venv").join("pyvenv.cfg");
        if !pyvenv.exists() {
            continue;
        }
        
        let content = fs::read_to_string(&pyvenv).map_err(|e| e.to_string())?;
        let mut new_content = content.clone();
        
        for (legacy, target) in &replacements {
            new_content = new_content.replace(legacy, target);
            
            #[cfg(windows)]
            {
                let legacy_forward = legacy.replace('\\', "/");
                let target_forward = target.replace('\\', "/");
                let legacy_backward = legacy.replace('/', "\\");
                let target_backward = target.replace('/', "\\");
                
                new_content = new_content.replace(&legacy_forward, &target_forward);
                new_content = new_content.replace(&legacy_backward, &target_backward);
            }
        }
        
        if new_content != content {
            fs::write(&pyvenv, new_content).map_err(|e| e.to_string())?;
            log::info!("Migration: updated {:?}", pyvenv);
        }
    }
    Ok(())
}
