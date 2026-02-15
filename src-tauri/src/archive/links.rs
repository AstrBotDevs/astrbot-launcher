use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{AppError, Result};

use super::path::{resolve_within_dir, validate_rel_link_target};

#[derive(Clone, Copy)]
enum SymlinkTargetKind {
    File,
    Dir,
}

pub(super) struct QueuedSymlink {
    pub(super) out_path: PathBuf,
    pub(super) target: PathBuf,
    pub(super) resolved_target: PathBuf,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn create_symlink(
    target: &Path,
    link_path: &Path,
    _target_kind: Option<SymlinkTargetKind>,
) -> Result<()> {
    std::os::unix::fs::symlink(target, link_path)
        .map_err(|e| AppError::io(format!("failed to create symlink at {link_path:?}: {e}")))
}

#[cfg(windows)]
fn create_symlink(
    target: &Path,
    link_path: &Path,
    target_kind: Option<SymlinkTargetKind>,
) -> Result<()> {
    let target_kind = target_kind.ok_or_else(|| {
        AppError::io(
            "cannot determine symlink type on Windows when target does not exist in archive",
        )
    })?;
    let result = match target_kind {
        SymlinkTargetKind::Dir => std::os::windows::fs::symlink_dir(target, link_path),
        SymlinkTargetKind::File => std::os::windows::fs::symlink_file(target, link_path),
    };
    result.map_err(|e| AppError::io(format!("failed to create symlink at {link_path:?}: {e}")))
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn create_symlink(
    _target: &Path,
    _link_path: &Path,
    _target_kind: Option<SymlinkTargetKind>,
) -> Result<()> {
    Err(AppError::io(
        "symlink creation not supported on this platform",
    ))
}

fn create_hard_link(target: &Path, link_path: &Path) -> Result<()> {
    if let Err(e) = fs::hard_link(target, link_path) {
        log::warn!("hard_link failed ({e}), falling back to copy");
        fs::copy(target, link_path).map_err(|e| {
            AppError::io(format!("failed to copy {target:?} to {link_path:?}: {e}"))
        })?;
    }
    Ok(())
}

fn resolve_relative_symlink_target(
    out_path: &Path,
    target: &Path,
    dest_dir: &Path,
) -> Result<PathBuf> {
    let parent = out_path
        .parent()
        .ok_or_else(|| AppError::io("symlink entry has no parent directory"))?;
    resolve_within_dir(dest_dir, &parent.join(target))
}

pub(super) fn queue_symlink(
    out_path: &Path,
    target: &Path,
    dest_dir: &Path,
) -> Result<QueuedSymlink> {
    validate_rel_link_target(target, "symlink")?;

    // Validate the target path upfront; actual symlink creation is deferred.
    let resolved_target = resolve_relative_symlink_target(out_path, target, dest_dir)?;
    Ok(QueuedSymlink {
        out_path: out_path.to_path_buf(),
        target: target.to_path_buf(),
        resolved_target,
    })
}

pub(super) fn create_queued_symlinks(pending: Vec<QueuedSymlink>) -> Result<()> {
    for item in pending {
        let parent = item
            .out_path
            .parent()
            .ok_or_else(|| AppError::io("symlink entry has no parent directory"))?;
        fs::create_dir_all(parent)
            .map_err(|e| AppError::io(format!("failed to create directory {parent:?}: {e}")))?;
        let target_kind = if item.resolved_target.exists() {
            if item.resolved_target.is_dir() {
                Some(SymlinkTargetKind::Dir)
            } else {
                Some(SymlinkTargetKind::File)
            }
        } else {
            None
        };
        create_symlink(&item.target, &item.out_path, target_kind)?;
    }

    Ok(())
}

pub(super) fn create_hard_link_entry(out_path: &Path, resolved_target: &Path) -> Result<()> {
    let parent = out_path
        .parent()
        .ok_or_else(|| AppError::io("entry has no parent directory"))?;
    fs::create_dir_all(parent)
        .map_err(|e| AppError::io(format!("failed to create directory {parent:?}: {e}")))?;
    create_hard_link(resolved_target, out_path)
}
