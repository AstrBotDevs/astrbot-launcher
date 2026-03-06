use std::path::{Path, PathBuf};

#[cfg(target_os = "windows")]
use crate::error::AppError;
use crate::error::Result;
#[cfg(target_os = "windows")]
use crate::process::win_api::{get_processes_locking_files, LockingProcessInfo};
#[cfg(target_os = "windows")]
use walkdir::WalkDir;

#[cfg(target_os = "windows")]
pub(crate) fn collect_files_for_lock_check(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    let mut iter = WalkDir::new(dir).into_iter();
    while let Some(entry) = iter.next() {
        let entry = entry.map_err(|e| AppError::io(e.to_string()))?;
        let path = entry.path();

        if entry.file_type().is_dir() && entry.file_name() == "__pycache__" {
            iter.skip_current_dir();
            continue;
        }

        if entry.file_type().is_file() && path.extension().map(|ext| ext != "pyc").unwrap_or(true) {
            files.push(entry.into_path());
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

#[cfg(target_os = "windows")]
fn query_lock_details(target_files: &[PathBuf]) -> Vec<String> {
    let mut lock_details = Vec::new();

    for target_file in target_files {
        let Ok(processes) = get_processes_locking_files(std::slice::from_ref(target_file)) else {
            continue;
        };
        if processes.is_empty() {
            continue;
        }

        let process_items = processes
            .iter()
            .map(format_locking_process)
            .collect::<Vec<_>>();

        lock_details.push(format!(
            "{} <- {}",
            target_file.display(),
            process_items.join("、")
        ));
    }

    lock_details
}

/// Ensure target files are not locked by other processes before mutating.
#[cfg(target_os = "windows")]
pub(crate) fn ensure_target_not_locked(target_files: &[PathBuf]) -> Result<()> {
    let locking_processes = get_processes_locking_files(target_files).map_err(|e| {
        log::warn!("Failed to query locking processes: {}", e);
        AppError::process_locking("目标路径占用状态检测失败")
    })?;
    if locking_processes.is_empty() {
        return Ok(());
    }

    let lock_details = query_lock_details(target_files);
    let process_items = locking_processes
        .into_iter()
        .map(|process| format_locking_process(&process))
        .collect::<Vec<_>>();
    let process_summary = process_items.join("；");
    let log_detail = if lock_details.is_empty() {
        process_summary
    } else {
        lock_details.join("；")
    };
    log::warn!("Target files are locked: {}", log_detail);
    Err(AppError::process_locking(
        "目标路径被占用，请关闭相关进程后重试，请前往日志页面查看详情。",
    ))
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn ensure_target_not_locked(_target_files: &[PathBuf]) -> Result<()> {
    Ok(())
}
