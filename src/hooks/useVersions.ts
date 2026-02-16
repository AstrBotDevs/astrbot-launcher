import { useCallback } from 'react';
import { message } from '../antdStatic';
import { api } from '../api';
import type { InstalledVersion, GitHubRelease } from '../types';
import { OPERATION_KEYS } from '../constants';
import { useAppStore } from '../stores';
import { SKIP_OPERATION, useOperationRunner } from './useOperationRunner';

interface UseVersionsReturn {
  handleInstall: (release: GitHubRelease) => Promise<void>;
  handleUninstall: (version: InstalledVersion) => Promise<void>;
}

export function useVersions(): UseVersionsReturn {
  const { runOperation } = useOperationRunner();

  const handleInstall = useCallback(
    async (release: GitHubRelease) => {
      const key = OPERATION_KEYS.installVersion(release.tag_name);
      await runOperation({
        key,
        reloadBefore: true,
        task: async () => {
          const { versions } = useAppStore.getState();
          if (versions.some((v) => v.version === release.tag_name)) {
            message.info(`版本 ${release.tag_name} 已下载`);
            return SKIP_OPERATION;
          }
          await api.installVersion(release);
        },
        onSuccess: () => {
          message.success(`版本 ${release.tag_name} 下载成功`);
        },
      });
    },
    [runOperation]
  );

  const handleUninstall = useCallback(
    async (version: InstalledVersion) => {
      const key = OPERATION_KEYS.uninstallVersion(version.version);
      await runOperation({
        key,
        reloadBefore: true,
        task: async () => {
          const { versions } = useAppStore.getState();
          if (!versions.some((v) => v.version === version.version)) {
            message.info(`版本 ${version.version} 已卸载`);
            return SKIP_OPERATION;
          }

          await api.uninstallVersion(version.version);
        },
        onSuccess: () => {
          message.success('已卸载');
        },
      });
    },
    [runOperation]
  );

  return {
    handleInstall,
    handleUninstall,
  };
}
