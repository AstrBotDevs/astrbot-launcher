use std::cmp::Ordering;
use std::sync::Arc;

use reqwest::Client;
use tauri::{AppHandle, State};

use crate::backup;
use crate::component;
use crate::component::ComponentsSnapshot;
use crate::config::{
    load_config, reload_config, with_config_mut, AppConfig, BackupInfo, InstalledVersion,
};
use crate::download;
use crate::error::{AppError, Result};
use crate::github::{self, GitHubRelease};
use crate::instance::{self, InstanceStatus, ProcessManager};
use crate::paths;
use crate::platform;

fn sort_installed_versions_semver(versions: &mut [InstalledVersion]) {
    versions.sort_by(|a, b| {
        let av = semver::Version::parse(a.version.trim_start_matches('v')).ok();
        let bv = semver::Version::parse(b.version.trim_start_matches('v')).ok();

        match (av, bv) {
            (Some(va), Some(vb)) => vb.cmp(&va),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => b.version.cmp(&a.version),
        }
    });
}

pub struct AppState {
    pub client: Client,
    pub process_manager: Arc<ProcessManager>,
}

fn apply_uv_fallback(config: &mut AppConfig) {
    if config.use_uv_for_deps && !component::is_uv_installed() {
        config.use_uv_for_deps = false;
        if let Err(e) = with_config_mut(|cfg| {
            if cfg.use_uv_for_deps {
                cfg.use_uv_for_deps = false;
            }
            Ok(())
        }) {
            log::warn!("Failed to persist uv fallback to pip: {}", e);
        }
    }
}

pub(crate) async fn build_app_snapshot_with(
    process_manager: &ProcessManager,
    load_config_fn: fn() -> Result<Arc<AppConfig>>,
) -> Result<AppSnapshot> {
    let config = load_config_fn()?;
    let instances = instance::list_instances(process_manager).await?;
    let backups = backup::list_backups()?;
    let mut config_for_snapshot = (*config).clone();
    apply_uv_fallback(&mut config_for_snapshot);
    sort_installed_versions_semver(&mut config_for_snapshot.installed_versions);

    Ok(AppSnapshot {
        instances,
        versions: config_for_snapshot.installed_versions.clone(),
        backups,
        components: component::build_components_snapshot(),
        config: config_for_snapshot,
    })
}

#[tauri::command]
pub async fn get_app_snapshot(state: State<'_, AppState>) -> Result<AppSnapshot> {
    build_app_snapshot_with(&state.process_manager, load_config).await
}

#[tauri::command]
pub async fn rebuild_app_snapshot(state: State<'_, AppState>) -> Result<AppSnapshot> {
    build_app_snapshot_with(&state.process_manager, reload_config).await
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AppSnapshot {
    pub instances: Vec<InstanceStatus>,
    pub versions: Vec<InstalledVersion>,
    pub backups: Vec<BackupInfo>,
    pub components: ComponentsSnapshot,
    pub config: AppConfig,
}

// === Config ===

#[tauri::command]
pub fn is_macos() -> bool {
    platform::is_macos()
}

#[tauri::command]
pub async fn save_github_proxy(github_proxy: String, state: State<'_, AppState>) -> Result<()> {
    // Test connectivity first
    let url = github::build_api_url(&github_proxy);
    download::check_url(&state.client, &url).await?;
    // Test passed, save
    with_config_mut(move |config| {
        config.github_proxy = github_proxy;
        Ok(())
    })
}

#[tauri::command]
pub async fn save_pypi_mirror(pypi_mirror: String, state: State<'_, AppState>) -> Result<()> {
    // Test connectivity first
    let base = if pypi_mirror.is_empty() {
        "https://pypi.org".to_string()
    } else {
        pypi_mirror.trim_end_matches('/').to_string()
    };
    let url = format!("{}/simple/", base);
    download::check_url(&state.client, &url).await?;
    // Test passed, save
    with_config_mut(move |config| {
        config.pypi_mirror = pypi_mirror;
        Ok(())
    })
}

#[tauri::command]
pub async fn save_close_to_tray(close_to_tray: bool) -> Result<()> {
    with_config_mut(move |config| {
        config.close_to_tray = close_to_tray;
        Ok(())
    })
}

#[tauri::command]
pub async fn save_nodejs_mirror(nodejs_mirror: String) -> Result<()> {
    with_config_mut(move |config| {
        config.nodejs_mirror = nodejs_mirror;
        Ok(())
    })
}

#[tauri::command]
pub async fn save_npm_registry(npm_registry: String) -> Result<()> {
    with_config_mut(move |config| {
        config.npm_registry = npm_registry;
        Ok(())
    })
}

#[tauri::command]
pub async fn save_use_uv_for_deps(use_uv_for_deps: bool) -> Result<()> {
    if use_uv_for_deps && !component::is_uv_installed() {
        return Err(AppError::other("uv 组件未安装，无法启用 uv 安装依赖"));
    }

    with_config_mut(move |config| {
        config.use_uv_for_deps = use_uv_for_deps;
        Ok(())
    })
}

#[tauri::command]
pub fn compare_versions(a: String, b: String) -> i32 {
    match (
        semver::Version::parse(a.trim_start_matches('v')),
        semver::Version::parse(b.trim_start_matches('v')),
    ) {
        (Ok(va), Ok(vb)) => va.cmp(&vb) as i32,
        _ => 0,
    }
}

#[tauri::command]
pub async fn save_check_instance_update(check_instance_update: bool) -> Result<()> {
    with_config_mut(move |config| {
        config.check_instance_update = check_instance_update;
        Ok(())
    })
}

#[tauri::command]
pub async fn save_persist_instance_state(persist_instance_state: bool) -> Result<()> {
    with_config_mut(move |config| {
        config.persist_instance_state = persist_instance_state;
        Ok(())
    })
}

// === Components ===

#[tauri::command]
pub fn is_instance_deployed(instance_id: &str) -> bool {
    paths::is_instance_deployed(instance_id)
}

#[tauri::command]
pub async fn install_component(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    component_id: String,
) -> Result<String> {
    let id = component::ComponentId::from_str_id(&component_id)
        .ok_or_else(|| AppError::other(format!("Unknown component: {}", component_id)))?;
    component::install_component(&state.client, id, Some(&app_handle)).await
}

#[tauri::command]
pub async fn reinstall_component(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    component_id: String,
) -> Result<String> {
    let id = component::ComponentId::from_str_id(&component_id)
        .ok_or_else(|| AppError::other(format!("Unknown component: {}", component_id)))?;
    component::reinstall_component(&state.client, id, Some(&app_handle)).await
}

// === GitHub ===

#[tauri::command]
pub async fn fetch_releases(
    state: State<'_, AppState>,
    force_refresh: Option<bool>,
) -> Result<Vec<GitHubRelease>> {
    github::fetch_releases(&state.client, force_refresh.unwrap_or(false)).await
}

// === Version Management ===

#[tauri::command]
pub async fn install_version(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    release: GitHubRelease,
) -> Result<()> {
    download::download_version(&state.client, &release, Some(&app_handle)).await
}

#[tauri::command]
pub async fn uninstall_version(version: String) -> Result<()> {
    download::remove_version(&version)
}

// === Troubleshooting ===

#[tauri::command]
pub async fn clear_instance_data(instance_id: String, state: State<'_, AppState>) -> Result<()> {
    if state.process_manager.is_running(&instance_id).await {
        return Err(AppError::instance_running());
    }
    instance::clear_instance_data(&instance_id)
}

#[tauri::command]
pub async fn clear_instance_venv(instance_id: String, state: State<'_, AppState>) -> Result<()> {
    if state.process_manager.is_running(&instance_id).await {
        return Err(AppError::instance_running());
    }
    instance::clear_instance_venv(&instance_id)
}

#[tauri::command]
pub async fn clear_pycache(instance_id: String, state: State<'_, AppState>) -> Result<()> {
    if state.process_manager.is_running(&instance_id).await {
        return Err(AppError::instance_running());
    }
    instance::clear_pycache(&instance_id)
}

// === Instance Management ===

#[tauri::command]
pub async fn create_instance(name: String, version: String, port: u16) -> Result<()> {
    instance::create_instance(&name, &version, port)
}

#[tauri::command]
pub async fn delete_instance(instance_id: String, state: State<'_, AppState>) -> Result<()> {
    instance::delete_instance(&instance_id, Arc::clone(&state.process_manager)).await
}

#[tauri::command]
pub async fn update_instance(
    app_handle: AppHandle,
    instance_id: String,
    name: Option<String>,
    version: Option<String>,
    port: Option<u16>,
    state: State<'_, AppState>,
) -> Result<()> {
    if state.process_manager.is_running(&instance_id).await {
        return Err(AppError::instance_running());
    }

    instance::update_instance(
        &instance_id,
        name.as_deref(),
        version.as_deref(),
        port,
        &app_handle,
    )
    .await
}

#[tauri::command]
pub async fn start_instance(
    app_handle: AppHandle,
    instance_id: String,
    state: State<'_, AppState>,
) -> Result<u16> {
    instance::start_instance(
        &instance_id,
        &app_handle,
        Arc::clone(&state.process_manager),
    )
    .await
}

#[tauri::command]
pub async fn stop_instance(instance_id: String, state: State<'_, AppState>) -> Result<()> {
    instance::stop_instance(&instance_id, Arc::clone(&state.process_manager)).await
}

#[tauri::command]
pub async fn restart_instance(
    app_handle: AppHandle,
    instance_id: String,
    state: State<'_, AppState>,
) -> Result<u16> {
    instance::restart_instance(
        &instance_id,
        &app_handle,
        Arc::clone(&state.process_manager),
    )
    .await
}

#[tauri::command]
pub async fn get_instance_port(instance_id: String, state: State<'_, AppState>) -> Result<u16> {
    state
        .process_manager
        .get_port(&instance_id)
        .ok_or_else(AppError::instance_not_running)
}

// === Backup ===

#[tauri::command]
pub async fn create_backup(instance_id: String, state: State<'_, AppState>) -> Result<String> {
    if state.process_manager.is_running(&instance_id).await {
        return Err(AppError::instance_running());
    }
    backup::create_backup(&instance_id, false)
}

#[tauri::command]
pub async fn restore_backup(backup_path: String) -> Result<()> {
    backup::restore_backup(&backup_path)
}

#[tauri::command]
pub async fn delete_backup(backup_path: String) -> Result<()> {
    backup::delete_backup(&backup_path)
}
