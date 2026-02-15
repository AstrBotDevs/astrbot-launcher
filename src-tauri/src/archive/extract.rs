use std::fs;
use std::io;
use std::path::Path;

use crate::error::{AppError, Result};

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn set_unix_permissions(path: &Path, mode: Option<u32>) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    if let Some(mode) = mode {
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .map_err(|e| AppError::io(format!("failed to set permissions on {path:?}: {e}")))?;
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn set_unix_permissions(_path: &Path, _mode: Option<u32>) -> Result<()> {
    Ok(())
}

pub(super) fn write_entry<R>(
    out_path: &Path,
    is_dir: bool,
    reader: &mut R,
    unix_mode: Option<u32>,
    declared_size: Option<u64>,
) -> Result<()>
where
    R: io::Read,
{
    if is_dir {
        fs::create_dir_all(out_path)
            .map_err(|e| AppError::io(format!("failed to create directory {out_path:?}: {e}")))?;
        return Ok(());
    }

    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| AppError::io(format!("failed to create directory {parent:?}: {e}")))?;
    }

    let mut outfile =
        fs::File::create(out_path).map_err(|error| AppError::io(error.to_string()))?;
    let written =
        io::copy(reader, &mut outfile).map_err(|error| AppError::io(error.to_string()))?;
    if let Some(expected_size) = declared_size {
        if written != expected_size {
            return Err(AppError::io(format!(
                "archive entry size mismatch: expected {expected_size} bytes, wrote {written} bytes",
            )));
        }
    }
    set_unix_permissions(out_path, unix_mode)?;
    Ok(())
}
