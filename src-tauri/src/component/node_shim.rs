//! Generate shim scripts that wrap node/npm/npx with the correct environment variables.
//!
//! On Unix, generates `#!/bin/sh` scripts.
//! On Windows, generates `.cmd` and `.ps1` scripts.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::error::{AppError, Result};
use crate::paths::{
    get_component_dir, get_node_bin_dir, get_node_exe_path, get_nodejs_shim_dir,
    get_npm_exe_path, get_npm_prefix_bin_dir, get_npm_prefix_modules_dir, get_npx_exe_path,
};

/// Tools for which shims are generated.
const SHIM_TOOLS: &[&str] = &["node", "npm", "npx"];

/// Generate shim scripts for node, npm, and npx.
///
/// Each shim sets the provided environment variables, prepends the correct
/// directories to PATH, then exec's the real binary.
/// Returns the shim directory path.
pub fn generate_shims(env_vars: &[(OsString, OsString)]) -> Result<PathBuf> {
    let shim_dir = get_nodejs_shim_dir();
    std::fs::create_dir_all(&shim_dir)
        .map_err(|e| AppError::io(format!("Failed to create shim dir: {}", e)))?;

    let nodejs_dir = get_component_dir("nodejs");
    let npm_prefix = &nodejs_dir;

    // Directories to prepend to PATH inside shims
    let modules_bin = get_npm_prefix_modules_dir(npm_prefix).join(".bin");
    let prefix_bin = get_npm_prefix_bin_dir(npm_prefix);
    let node_bin = get_node_bin_dir(&nodejs_dir);

    // Real binary paths
    let real_node = get_node_exe_path(&nodejs_dir);
    let real_npm = get_npm_exe_path(&nodejs_dir);
    let real_npx = get_npx_exe_path(&nodejs_dir);

    let real_bins: [&std::path::Path; 3] = [&real_node, &real_npm, &real_npx];

    for (tool, real_bin) in SHIM_TOOLS.iter().zip(real_bins.iter()) {
        generate_platform_shims(
            &shim_dir,
            tool,
            real_bin,
            env_vars,
            &modules_bin,
            &prefix_bin,
            &node_bin,
        )?;
    }

    Ok(shim_dir)
}

fn generate_platform_shims(
    shim_dir: &std::path::Path,
    tool: &str,
    real_bin: &std::path::Path,
    env_vars: &[(OsString, OsString)],
    modules_bin: &std::path::Path,
    prefix_bin: &std::path::Path,
    node_bin: &std::path::Path,
) -> Result<()> {
    #[cfg(not(target_os = "windows"))]
    {
        generate_sh_shim(shim_dir, tool, real_bin, env_vars, modules_bin, prefix_bin, node_bin)?;
    }

    #[cfg(target_os = "windows")]
    {
        generate_cmd_shim(shim_dir, tool, real_bin, env_vars, modules_bin, prefix_bin, node_bin)?;
        generate_ps1_shim(shim_dir, tool, real_bin, env_vars, modules_bin, prefix_bin, node_bin)?;
    }

    Ok(())
}

// ── Unix: sh shim ──────────────────────────────────────────────────────────

#[cfg(not(target_os = "windows"))]
fn generate_sh_shim(
    shim_dir: &std::path::Path,
    tool: &str,
    real_bin: &std::path::Path,
    env_vars: &[(OsString, OsString)],
    modules_bin: &std::path::Path,
    prefix_bin: &std::path::Path,
    node_bin: &std::path::Path,
) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;

    let mut script = String::from("#!/bin/sh\n");

    // Export environment variables
    for (key, val) in env_vars {
        let key_str = String::from_utf8_lossy(key.as_bytes());
        let val_str = String::from_utf8_lossy(val.as_bytes());
        script.push_str(&format!("export {}='{}'\n", key_str, shell_escape(&val_str)));
    }

    // Prepend PATH
    script.push_str(&format!(
        "export PATH='{}':'{}':'{}':\"$PATH\"\n",
        shell_escape(&modules_bin.display().to_string()),
        shell_escape(&prefix_bin.display().to_string()),
        shell_escape(&node_bin.display().to_string()),
    ));

    // exec real binary
    script.push_str(&format!(
        "exec '{}' \"$@\"\n",
        shell_escape(&real_bin.display().to_string()),
    ));

    let path = shim_dir.join(tool);
    std::fs::write(&path, &script)
        .map_err(|e| AppError::io(format!("Failed to write shim {}: {}", path.display(), e)))?;

    // chmod +x
    set_executable(&path)?;

    Ok(())
}

// ── Windows: cmd shim ──────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn generate_cmd_shim(
    shim_dir: &std::path::Path,
    tool: &str,
    real_bin: &std::path::Path,
    env_vars: &[(OsString, OsString)],
    modules_bin: &std::path::Path,
    prefix_bin: &std::path::Path,
    node_bin: &std::path::Path,
) -> Result<()> {
    let mut script = String::from("@echo off\r\n");

    for (key, val) in env_vars {
        let key_str = key.to_string_lossy();
        let val_str = val.to_string_lossy();
        script.push_str(&format!("set \"{}={}\"\r\n", key_str, val_str));
    }

    script.push_str(&format!(
        "set \"PATH={};{};{};%PATH%\"\r\n",
        modules_bin.display(),
        prefix_bin.display(),
        node_bin.display(),
    ));

    script.push_str(&format!("\"{}\" %*\r\n", real_bin.display()));

    let path = shim_dir.join(format!("{}.cmd", tool));
    std::fs::write(&path, &script)
        .map_err(|e| AppError::io(format!("Failed to write shim {}: {}", path.display(), e)))?;

    Ok(())
}

// ── Windows: ps1 shim ──────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn generate_ps1_shim(
    shim_dir: &std::path::Path,
    tool: &str,
    real_bin: &std::path::Path,
    env_vars: &[(OsString, OsString)],
    modules_bin: &std::path::Path,
    prefix_bin: &std::path::Path,
    node_bin: &std::path::Path,
) -> Result<()> {
    let mut script = String::new();

    for (key, val) in env_vars {
        let key_str = key.to_string_lossy();
        let val_str = val.to_string_lossy();
        script.push_str(&format!(
            "$env:{} = '{}'\r\n",
            key_str,
            val_str.replace('\'', "''")
        ));
    }

    script.push_str(&format!(
        "$env:PATH = '{};{};{};' + $env:PATH\r\n",
        modules_bin.display(),
        prefix_bin.display(),
        node_bin.display(),
    ));

    script.push_str(&format!(
        "& '{}' @args\r\n",
        real_bin.display().to_string().replace('\'', "''")
    ));

    let path = shim_dir.join(format!("{}.ps1", tool));
    std::fs::write(&path, &script)
        .map_err(|e| AppError::io(format!("Failed to write shim {}: {}", path.display(), e)))?;

    Ok(())
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Escape single quotes for sh scripts.
fn shell_escape(s: &str) -> String {
    s.replace('\'', "'\\''")
}

#[cfg(not(target_os = "windows"))]
fn set_executable(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)
        .map_err(|e| AppError::io(format!("Failed to read permissions for {:?}: {}", path, e)))?
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)
        .map_err(|e| AppError::io(format!("Failed to set permissions for {:?}: {}", path, e)))?;
    Ok(())
}
