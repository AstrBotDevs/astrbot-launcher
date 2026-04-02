use std::path::{Path, PathBuf};

#[cfg(target_os = "windows")]
use crate::error::AppError;
use crate::error::Result;
#[cfg(target_os = "windows")]
use crate::process::win_api::{
    get_processes_locking_files, LockingProcessInfo, RestartManagerQueryError,
};
#[cfg(target_os = "windows")]
use walkdir::WalkDir;

/// Directory names to skip when collecting files for lock checks.
const SKIP_DIRS: &[&str] = &[".git", "node_modules", "dist", "__pycache__"];
/// When extension whitelist mode is enabled, only files with these extensions are registered.
const EXTENSION_WHITELIST: &[&str] = &[
    "py", "js", "ts", "db", "db-shm", "db-wal", "json", "md", "html", "sh", "ps1", "bat", "cmd",
    "fish",
];

#[cfg(target_os = "windows")]
pub(crate) fn collect_files_for_lock_check(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let use_whitelist = crate::config::load_config()
        .map(|c| c.lock_check_extension_whitelist)
        .unwrap_or(false);

    let mut files = Vec::new();
    let mut iter = WalkDir::new(dir).into_iter();
    while let Some(entry) = iter.next() {
        let entry = entry.map_err(|e| AppError::io(e.to_string()))?;
        let path = entry.path();

        if entry.file_type().is_dir()
            && entry
                .file_name()
                .to_str()
                .is_some_and(|name| SKIP_DIRS.contains(&name))
        {
            iter.skip_current_dir();
            continue;
        }

        if entry.file_type().is_file() {
            let ext_match = if use_whitelist {
                path.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|ext| {
                        EXTENSION_WHITELIST.contains(&ext.to_ascii_lowercase().as_str())
                    })
            } else {
                path.extension().map(|ext| ext != "pyc").unwrap_or(true)
            };
            if ext_match {
                files.push(entry.into_path());
            }
        }
    }

    Ok(files)
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn collect_files_for_lock_check(_dir: &Path) -> Result<Vec<PathBuf>> {
    Ok(Vec::new())
}

#[cfg(target_os = "windows")]
fn format_locking_process(process: &LockingProcessInfo) -> String {
    let mut labels = Vec::new();
    if let Some(path) = &process.executable_path {
        labels.push(path.display().to_string());
    } else if !process.app_name.is_empty() {
        labels.push(process.app_name.clone());
    }
    if !process.service_short_name.is_empty() {
        labels.push(format!("服务：{}", process.service_short_name.clone()));
    }

    if labels.is_empty() {
        format!("PID {}", process.pid)
    } else {
        format!("PID {} ({})", process.pid, labels.join(", "))
    }
}

/// Ensure target files are not locked by other processes before mutating.
///
/// Single batch Restart Manager query for all files.
#[cfg(target_os = "windows")]
pub(crate) fn ensure_target_not_locked(target_files: &[PathBuf]) -> Result<()> {
    let locking_processes = get_processes_locking_files(target_files).map_err(|e| {
        log::warn!("Failed to query locking processes: {}", e);
        if matches!(e, RestartManagerQueryError::MaxSessionsReached) {
            AppError::process_locking("系统 Restart Manager 会话数已达上限")
        } else {
            AppError::process_locking("目标路径占用状态检测失败")
        }
    })?;
    if locking_processes.is_empty() {
        return Ok(());
    }

    let process_items: Vec<String> = locking_processes
        .iter()
        .map(format_locking_process)
        .collect();
    log::warn!("Target files are locked by: {}", process_items.join("；"));
    Err(AppError::process_locking(
        "目标路径被占用，请关闭相关进程后重试，请前往日志页面查看详情。",
    ))
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn ensure_target_not_locked(_target_files: &[PathBuf]) -> Result<()> {
    Ok(())
}
