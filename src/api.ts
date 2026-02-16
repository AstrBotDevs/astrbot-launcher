import { invoke } from '@tauri-apps/api/core';
import type { GitHubRelease, AppSnapshot } from './types';

// Re-export types for convenience
export type {
  AppSnapshot,
  AppConfig,
  InstanceConfig,
  InstanceStatus,
  InstalledVersion,
  GitHubRelease,
  GitHubAsset,
  BackupMetadata,
  BackupInfo,
  DeployProgress,
  DeployStep,
  DeployType,
  ComponentStatus,
  ComponentsSnapshot,
} from './types';

export const api = {
  // ========================================
  // Snapshot
  // ========================================
  getAppSnapshot: () => invoke<AppSnapshot>('get_app_snapshot'),
  rebuildAppSnapshot: () => invoke<AppSnapshot>('rebuild_app_snapshot'),

  // ========================================
  // Config
  // ========================================
  saveGithubProxy: (githubProxy: string) => invoke<void>('save_github_proxy', { githubProxy }),
  savePypiMirror: (pypiMirror: string) => invoke<void>('save_pypi_mirror', { pypiMirror }),
  saveNodejsMirror: (nodejsMirror: string) => invoke<void>('save_nodejs_mirror', { nodejsMirror }),
  saveNpmRegistry: (npmRegistry: string) => invoke<void>('save_npm_registry', { npmRegistry }),
  saveUseUvForDeps: (useUvForDeps: boolean) =>
    invoke<void>('save_use_uv_for_deps', { useUvForDeps }),
  saveCloseToTray: (closeToTray: boolean) => invoke<void>('save_close_to_tray', { closeToTray }),
  compareVersions: (a: string, b: string) => invoke<number>('compare_versions', { a, b }),
  saveCheckInstanceUpdate: (checkInstanceUpdate: boolean) =>
    invoke<void>('save_check_instance_update', { checkInstanceUpdate }),
  savePersistInstanceState: (persistInstanceState: boolean) =>
    invoke<void>('save_persist_instance_state', { persistInstanceState }),
  isMacOS: () => invoke<boolean>('is_macos'),

  // ========================================
  // Components
  // ========================================
  installComponent: (componentId: string) => invoke<string>('install_component', { componentId }),
  reinstallComponent: (componentId: string) =>
    invoke<string>('reinstall_component', { componentId }),

  // ========================================
  // GitHub
  // ========================================
  fetchReleases: () => invoke<GitHubRelease[]>('fetch_releases'),

  // ========================================
  // Version Management
  // ========================================
  installVersion: (release: GitHubRelease) => invoke<void>('install_version', { release }),
  uninstallVersion: (version: string) => invoke<void>('uninstall_version', { version }),

  // ========================================
  // Troubleshooting
  // ========================================
  clearInstanceData: (instanceId: string) => invoke<void>('clear_instance_data', { instanceId }),
  clearInstanceVenv: (instanceId: string) => invoke<void>('clear_instance_venv', { instanceId }),
  clearPycache: (instanceId: string) => invoke<void>('clear_pycache', { instanceId }),

  // ========================================
  // Instance Management
  // ========================================
  createInstance: (name: string, version: string, port: number = 0) =>
    invoke<void>('create_instance', { name, version, port }),
  deleteInstance: (instanceId: string) => invoke<void>('delete_instance', { instanceId }),
  updateInstance: (instanceId: string, name?: string, version?: string, port?: number) =>
    invoke<void>('update_instance', {
      instanceId,
      name: name ?? null,
      version: version ?? null,
      port: port ?? null,
    }),
  isInstanceDeployed: (instanceId: string) =>
    invoke<boolean>('is_instance_deployed', { instanceId }),
  startInstance: (instanceId: string) => invoke<number>('start_instance', { instanceId }),
  stopInstance: (instanceId: string) => invoke<void>('stop_instance', { instanceId }),
  restartInstance: (instanceId: string) => invoke<number>('restart_instance', { instanceId }),
  getInstancePort: (instanceId: string) => invoke<number>('get_instance_port', { instanceId }),

  // ========================================
  // Backup
  // ========================================
  createBackup: (instanceId: string) => invoke<string>('create_backup', { instanceId }),
  restoreBackup: (backupPath: string) => invoke<void>('restore_backup', { backupPath }),
  deleteBackup: (backupPath: string) => invoke<void>('delete_backup', { backupPath }),
};
