use std::fs;
use std::path::{Path, PathBuf};

use crate::paths::{get_component_dir, get_data_dir};

/// Migrate legacy `python/` and `compat_python/` directories to the unified
/// `components/python/py312` and `components/python/py310` layout.
///
/// Migration errors are logged but never crash the app.
pub fn migrate_legacy_python_dirs() {
    let data_dir = get_data_dir();

    migrate_dir(
        &data_dir.join("python"),
        &get_component_dir("python").join("py312"),
        "python/ -> components/python/py312",
    );

    migrate_dir(
        &data_dir.join("compat_python"),
        &get_component_dir("python").join("py310"),
        "compat_python/ -> components/python/py310",
    );

    if let Err(e) = migrate_instance_pyvenv_cfgs(&data_dir) {
        log::warn!(
            "Migration: failed to update instance pyvenv.cfg files: {}",
            e
        );
    }
}

/// On Windows ARM, remove legacy ARM64 Python runtimes so the launcher can
/// reinstall x86_64 runtimes on demand.
///
/// Migration errors are logged but never crash the app.
#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
pub fn migrate_windows_arm_python_component_if_needed() {
    let python_component_dir = get_component_dir("python");
    if !python_component_dir.exists() {
        return;
    }

    let py310_dir = python_component_dir.join("py310");
    let py312_dir = python_component_dir.join("py312");

    let need_py310 = runtime_needs_migration(&py310_dir);
    let need_py312 = runtime_needs_migration(&py312_dir);
    if !need_py310 && !need_py312 {
        return;
    }

    log::info!(
        "Migration: detected incompatible Python runtime on Windows ARM (py310: {}, py312: {}), removing python component for x86_64 reinstall",
        need_py310,
        need_py312
    );

    if let Err(e) = clear_instance_venvs_for_python_migration(&get_data_dir()) {
        log::warn!(
            "Migration: failed to clean instance venvs before python component migration: {}",
            e
        );
    }

    if let Err(e) = fs::remove_dir_all(&python_component_dir) {
        log::warn!(
            "Migration: failed to remove python component at {:?}: {}",
            python_component_dir,
            e
        );
        return;
    }

    log::info!(
        "Migration: removed python component at {:?}",
        python_component_dir
    );
}

#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
fn runtime_needs_migration(runtime_dir: &Path) -> bool {
    const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;
    const IMAGE_FILE_MACHINE_ARM64: u16 = 0xAA64;

    if !runtime_dir.exists() {
        return false;
    }

    let python_exe = crate::paths::get_python_exe_path(runtime_dir);
    if !python_exe.exists() {
        log::warn!(
            "Migration: python executable missing in runtime {:?}, migration required",
            runtime_dir
        );
        return true;
    }

    match read_pe_machine(&python_exe) {
        Ok(IMAGE_FILE_MACHINE_AMD64) => false,
        Ok(IMAGE_FILE_MACHINE_ARM64) => true,
        Ok(other) => {
            log::warn!(
                "Migration: unexpected PE machine type 0x{:04X} for {:?}, migration required",
                other,
                python_exe
            );
            true
        }
        Err(e) => {
            log::warn!(
                "Migration: failed to read PE machine for {:?}: {}, migration required",
                python_exe,
                e
            );
            true
        }
    }
}

#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
fn read_pe_machine(exe_path: &Path) -> Result<u16, String> {
    let bytes = fs::read(exe_path)
        .map_err(|e| format!("Failed to read executable {:?}: {}", exe_path, e))?;

    if bytes.len() < 0x40 {
        return Err("File too small for DOS header".to_string());
    }

    if &bytes[0..2] != b"MZ" {
        return Err("Invalid DOS signature".to_string());
    }

    let e_lfanew = u32::from_le_bytes([bytes[0x3C], bytes[0x3D], bytes[0x3E], bytes[0x3F]]);
    let nt_offset =
        usize::try_from(e_lfanew).map_err(|_| "NT header offset overflow".to_string())?;

    let pe_sig_end = nt_offset
        .checked_add(4)
        .ok_or_else(|| "NT header offset overflow".to_string())?;
    let machine_end = nt_offset
        .checked_add(6)
        .ok_or_else(|| "COFF header offset overflow".to_string())?;

    if machine_end > bytes.len() {
        return Err("NT/COFF header out of bounds".to_string());
    }

    if &bytes[nt_offset..pe_sig_end] != b"PE\0\0" {
        return Err("Invalid NT signature".to_string());
    }

    Ok(u16::from_le_bytes([
        bytes[nt_offset + 4],
        bytes[nt_offset + 5],
    ]))
}

#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
fn clear_instance_venvs_for_python_migration(data_dir: &Path) -> Result<(), String> {
    let instances_dir = data_dir.join("instances");
    if !instances_dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(&instances_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let entry_path = entry.path();
        if !entry_path.is_dir() {
            continue;
        }

        let venv_dir = entry_path.join("venv");
        if !venv_dir.exists() {
            continue;
        }

        match fs::remove_dir_all(&venv_dir) {
            Ok(_) => log::info!("Migration: removed instance venv {:?}", venv_dir),
            Err(e) => log::warn!(
                "Migration: failed to remove instance venv {:?}: {}",
                venv_dir,
                e
            ),
        }
    }

    Ok(())
}

fn migrate_dir(src: &Path, dst: &Path, label: &str) {
    if !src.exists() {
        return;
    }
    if dst.exists() {
        // Destination already exists — remove the legacy source to clean up.
        log::info!(
            "Migration {}: destination already exists, removing legacy dir",
            label
        );
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
    let target_python312 = to_absolute(get_component_dir("python").join("py312"));
    let target_python310 = to_absolute(get_component_dir("python").join("py310"));

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
