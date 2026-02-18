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

    if let Err(e) = clear_tracked_instances_snapshot_for_python_migration() {
        log::warn!(
            "Migration: failed to clear tracked instance snapshot before python component migration: {}",
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

    if let Err(e) = clear_instance_venvs_for_python_migration(&get_data_dir()) {
        log::warn!(
            "Migration: failed to clean instance venvs after python component migration: {}",
            e
        );
    }
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
            "Migration: python executable missing in runtime {:?}, skip migration because architecture cannot be determined",
            runtime_dir
        );
        return false;
    }

    match read_pe_machine(&python_exe) {
        Ok(IMAGE_FILE_MACHINE_AMD64) => false,
        Ok(IMAGE_FILE_MACHINE_ARM64) => true,
        Ok(other) => {
            log::warn!(
                "Migration: unexpected PE machine type 0x{:04X} for {:?}, skip migration",
                other,
                python_exe
            );
            false
        }
        Err(e) => {
            log::warn!(
                "Migration: failed to read PE machine for {:?}: {}, skip migration",
                python_exe,
                e
            );
            false
        }
    }
}

#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
fn read_pe_machine(exe_path: &Path) -> Result<u16, String> {
    use std::io::{ErrorKind, Read, Seek, SeekFrom};

    let mut file = fs::File::open(exe_path)
        .map_err(|e| format!("Failed to open executable {:?}: {}", exe_path, e))?;

    let mut dos_header = [0u8; 0x40];
    if let Err(e) = file.read_exact(&mut dos_header) {
        if e.kind() == ErrorKind::UnexpectedEof {
            return Err("File too small for DOS header".to_string());
        }
        return Err(format!(
            "Failed to read DOS header from {:?}: {}",
            exe_path, e
        ));
    }

    if &dos_header[0..2] != b"MZ" {
        return Err("Invalid DOS signature".to_string());
    }

    let nt_offset = u64::from(u32::from_le_bytes([
        dos_header[0x3C],
        dos_header[0x3D],
        dos_header[0x3E],
        dos_header[0x3F],
    ]));

    file.seek(SeekFrom::Start(nt_offset))
        .map_err(|e| format!("Failed to seek to NT header at {:?}: {}", exe_path, e))?;

    let mut nt_and_coff_prefix = [0u8; 6];
    if let Err(e) = file.read_exact(&mut nt_and_coff_prefix) {
        if e.kind() == ErrorKind::UnexpectedEof {
            return Err("NT/COFF header out of bounds".to_string());
        }
        return Err(format!(
            "Failed to read NT/COFF header from {:?}: {}",
            exe_path, e
        ));
    }

    if &nt_and_coff_prefix[0..4] != b"PE\0\0" {
        return Err("Invalid NT signature".to_string());
    }

    Ok(u16::from_le_bytes([
        nt_and_coff_prefix[4],
        nt_and_coff_prefix[5],
    ]))
}

#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
fn clear_tracked_instances_snapshot_for_python_migration() -> Result<(), String> {
    crate::config::with_config_mut(|config| {
        if !config.tracked_instances_snapshot.is_empty() {
            config.tracked_instances_snapshot.clear();
            log::info!(
                "Migration: cleared tracked_instances_snapshot due to Windows ARM Python migration"
            );
        }
        Ok(())
    })
    .map_err(|e| e.to_string())
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
