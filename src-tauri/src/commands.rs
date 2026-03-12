use std::cmp::Ordering;
use std::sync::RwLock;

use reqwest::Client;
use tauri::{AppHandle, State};

use crate::backup;
use crate::component;
use crate::component::ComponentsSnapshot;
use crate::config::{
    load_config, load_manifest, reload_config, reload_manifest, with_config_mut, AppConfig,
    AppManifest, BackupInfo, InstalledVersion,
};
use crate::download;
use crate::error::{AppError, Result};
use crate::github::{self, GitHubRelease};
use crate::instance::{self, InstanceStatus};
use crate::platform;
use crate::process::ProcessManager;
use crate::utils::index_url::normalize_default_index;
use crate::utils::net::{build_http_client_with_proxy, check_url};
use crate::utils::proxy::{
    build_single_url_proxy_settings, resolve_proxy_with_fallbacks, ProxyFields, ProxySource,
    DEFAULT_NO_PROXY_VALUE,
};
use crate::utils::sync::{read_lock_recover, write_lock_recover};

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
    pub client: RwLock<Client>,
    pub process_manager: ProcessManager,
}

impl AppState {
    fn client(&self) -> Client {
        read_lock_recover(&self.client, "AppState.client").clone()
    }

    fn replace_client(&self, client: Client) {
        *write_lock_recover(&self.client, "AppState.client") = client;
    }
}

macro_rules! define_save_config_command {
    ($fn_name:ident, $param:ident : $ty:ty, $field:ident) => {
        #[tauri::command]
        pub async fn $fn_name($param: $ty) -> Result<()> {
            with_config_mut(move |config| {
                config.$field = $param;
                Ok(())
            })
        }
    };
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

pub(crate) fn build_app_snapshot_with(
    process_manager: &ProcessManager,
    load_config_fn: fn() -> Result<std::sync::Arc<AppConfig>>,
    load_manifest_fn: fn() -> Result<std::sync::Arc<AppManifest>>,
) -> Result<AppSnapshot> {
    let config = load_config_fn()?;
    let manifest = load_manifest_fn()?;
    let instances = instance::list_instances(process_manager, manifest.as_ref());
    let backups = backup::list_backups()?;
    let mut config_for_snapshot = (*config).clone();
    apply_uv_fallback(&mut config_for_snapshot);
    let mut versions = manifest.installed_versions.clone();
    sort_installed_versions_semver(&mut versions);

    Ok(AppSnapshot {
        instances,
        versions,
        backups,
        components: component::build_components_snapshot(),
        config: config_for_snapshot,
    })
}

#[tauri::command]
pub async fn get_app_snapshot(state: State<'_, AppState>) -> Result<AppSnapshot> {
    let pm = state.process_manager.clone();
    tokio::task::spawn_blocking(move || build_app_snapshot_with(&pm, load_config, load_manifest))
        .await
        .map_err(|e| AppError::process(format!("Snapshot task panicked: {}", e)))?
}

#[tauri::command]
pub async fn rebuild_app_snapshot(state: State<'_, AppState>) -> Result<AppSnapshot> {
    let pm = state.process_manager.clone();
    tokio::task::spawn_blocking(move || {
        build_app_snapshot_with(&pm, reload_config, reload_manifest)
    })
    .await
    .map_err(|e| AppError::process(format!("Snapshot task panicked: {}", e)))?
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
    let client = state.client();
    check_url(&client, &url).await?;
    // Test passed, save
    with_config_mut(move |config| {
        config.github_proxy = github_proxy;
        Ok(())
    })
}

#[tauri::command]
pub async fn save_proxy(
    proxy_url: String,
    proxy_port: String,
    proxy_username: String,
    proxy_password: String,
    state: State<'_, AppState>,
) -> Result<()> {
    let proxy_fields = ProxyFields::new(proxy_url, proxy_port, proxy_username, proxy_password);

    // Use a raw client (no system-proxy fallback) purely for connectivity tests.
    let configured_proxy = build_single_url_proxy_settings(
        ProxySource::AppConfig,
        &proxy_fields,
        Some(DEFAULT_NO_PROXY_VALUE.to_string()),
    )?;
    let test_proxy = configured_proxy.clone();
    let test_client = build_http_client_with_proxy(test_proxy)?;

    if !proxy_fields.url.is_empty()
        && check_url(&test_client, "https://cloudflare.com/cdn-cgi/trace")
            .await
            .is_err()
        && check_url(&test_client, "https://baidu.com").await.is_err()
    {
        return Err(AppError::network(
            "代理配置错误，无法连接 cloudflare.com 或 baidu.com",
        ));
    }

    // Build the client with proxy priority: app config > environment > system > no proxy.
    let client = build_http_client_with_proxy(resolve_proxy_with_fallbacks(configured_proxy))?;

    let previous_client = state.client();
    state.replace_client(client);

    if let Err(error) = with_config_mut(move |config| {
        config.proxy_url = proxy_fields.url;
        config.proxy_port = proxy_fields.port;
        config.proxy_username = proxy_fields.username;
        config.proxy_password = proxy_fields.password;
        Ok(())
    }) {
        state.replace_client(previous_client);
        return Err(error);
    }

    Ok(())
}

#[tauri::command]
pub async fn save_pypi_mirror(pypi_mirror: String, state: State<'_, AppState>) -> Result<()> {
    let normalized_default_index = normalize_default_index(&pypi_mirror);
    let check_url_value = format!("{}/", normalized_default_index);

    // Test connectivity first
    let client = state.client();
    check_url(&client, &check_url_value).await?;

    // Test passed, save
    let normalized_for_save = if pypi_mirror.trim().is_empty() {
        String::new()
    } else {
        normalized_default_index
    };
    with_config_mut(move |config| {
        config.pypi_mirror = normalized_for_save;
        Ok(())
    })
}

define_save_config_command!(save_close_to_tray, close_to_tray: bool, close_to_tray);
define_save_config_command!(save_nodejs_mirror, nodejs_mirror: String, nodejs_mirror);
define_save_config_command!(save_npm_registry, npm_registry: String, npm_registry);

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

define_save_config_command!(
    save_check_instance_update,
    check_instance_update: bool,
    check_instance_update
);
define_save_config_command!(
    save_persist_instance_state,
    persist_instance_state: bool,
    persist_instance_state
);
define_save_config_command!(
    save_ignore_external_path,
    ignore_external_path: bool,
    ignore_external_path
);

// === Components ===

enum ComponentCommandAction {
    Install,
    Reinstall,
}

async fn run_component_command(
    app_handle: &AppHandle,
    state: &State<'_, AppState>,
    component_id: &str,
    action: ComponentCommandAction,
) -> Result<String> {
    let client = state.client();
    let id = component::ComponentId::from_str_id(component_id)
        .ok_or_else(|| AppError::other(format!("Unknown component: {}", component_id)))?;

    match action {
        ComponentCommandAction::Install => {
            component::install_component(&client, id, Some(app_handle)).await
        }
        ComponentCommandAction::Reinstall => {
            component::reinstall_component(&client, id, Some(app_handle)).await
        }
    }
}

#[tauri::command]
pub async fn install_component(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    component_id: String,
) -> Result<String> {
    run_component_command(
        &app_handle,
        &state,
        &component_id,
        ComponentCommandAction::Install,
    )
    .await
}

#[tauri::command]
pub async fn reinstall_component(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    component_id: String,
) -> Result<String> {
    run_component_command(
        &app_handle,
        &state,
        &component_id,
        ComponentCommandAction::Reinstall,
    )
    .await
}

// === GitHub ===

#[tauri::command]
pub async fn fetch_releases(
    state: State<'_, AppState>,
    force_refresh: Option<bool>,
) -> Result<Vec<GitHubRelease>> {
    let client = state.client();
    github::fetch_releases(&client, force_refresh.unwrap_or(false)).await
}

// === Version Management ===

#[tauri::command]
pub async fn install_version(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    release: GitHubRelease,
) -> Result<()> {
    let client = state.client();
    download::download_version(&client, &release, Some(&app_handle)).await
}

#[tauri::command]
pub async fn uninstall_version(version: String) -> Result<()> {
    download::remove_version(&version)
}

// === Troubleshooting ===

#[tauri::command]
pub async fn clear_instance_data(instance_id: String, state: State<'_, AppState>) -> Result<()> {
    let _guard = state.process_manager.acquire_guard(&instance_id)?;
    instance::clear_instance_data(&instance_id)
}

#[tauri::command]
pub async fn clear_instance_venv(instance_id: String, state: State<'_, AppState>) -> Result<()> {
    let _guard = state.process_manager.acquire_guard(&instance_id)?;
    instance::clear_instance_venv(&instance_id)
}

#[tauri::command]
pub async fn clear_pycache(instance_id: String, state: State<'_, AppState>) -> Result<()> {
    let _guard = state.process_manager.acquire_guard(&instance_id)?;
    instance::clear_pycache(&instance_id)
}

// === Instance Management ===

#[tauri::command]
pub async fn create_instance(name: String, version: String, port: u16) -> Result<()> {
    instance::create_instance(&name, &version, port)
}

#[tauri::command]
pub async fn delete_instance(instance_id: String, state: State<'_, AppState>) -> Result<()> {
    let _guard = state.process_manager.acquire_guard(&instance_id)?;
    instance::delete_instance(&instance_id)
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
    let _guard = state.process_manager.acquire_guard(&instance_id)?;
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
    state
        .process_manager
        .start_instance(&instance_id, app_handle)
        .await
}

#[tauri::command]
pub async fn stop_instance(instance_id: String, state: State<'_, AppState>) -> Result<()> {
    state.process_manager.stop_instance(&instance_id).await
}

#[tauri::command]
pub async fn restart_instance(
    app_handle: AppHandle,
    instance_id: String,
    state: State<'_, AppState>,
) -> Result<u16> {
    state
        .process_manager
        .restart_instance(&instance_id, app_handle)
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
    let _guard = state.process_manager.acquire_guard(&instance_id)?;
    backup::create_backup(&instance_id, false)
}

#[tauri::command]
pub async fn restore_backup(backup_path: String, state: State<'_, AppState>) -> Result<()> {
    let (resolved_path, metadata) = backup::resolve_restore_backup_input(&backup_path)?;
    let _guard = state.process_manager.acquire_guard(&metadata.instance_id)?;
    backup::restore_backup_with_input(resolved_path, metadata)
}

#[tauri::command]
pub async fn delete_backup(backup_path: String) -> Result<()> {
    backup::delete_backup(&backup_path)
}
