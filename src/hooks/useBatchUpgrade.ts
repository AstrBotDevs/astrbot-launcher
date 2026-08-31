import { useCallback, useRef, useState } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { api } from '../api';
import { message } from '../antdStatic';
import { useAppStore } from '../stores';
import { handleApiError } from '../utils';
import type { DeployProgress, GitHubRelease, InstanceStatus } from '../types';

export interface BatchUpgradeState {
  open: boolean;
  total: number;
  currentIndex: number;
  currentInstanceName: string;
  step: string | null;
  progress: number;
  error: string | null;
}

const INITIAL_STATE: BatchUpgradeState = {
  open: false,
  total: 0,
  currentIndex: 0,
  currentInstanceName: '',
  step: null,
  progress: 0,
  error: null,
};

export function useBatchUpgrade(releases: GitHubRelease[]) {
  const [state, setState] = useState<BatchUpgradeState>(INITIAL_STATE);
  const unlistenRef = useRef<UnlistenFn | null>(null);
  const instancesRef = useRef<InstanceStatus[]>([]);
  const latestVersionRef = useRef<string | null>(null);

  const cleanupListener = useCallback(() => {
    if (unlistenRef.current) {
      unlistenRef.current();
      unlistenRef.current = null;
    }
  }, []);

  const open = useCallback((instances: InstanceStatus[], latestVersion: string) => {
    instancesRef.current = instances;
    latestVersionRef.current = latestVersion;
    setState({
      ...INITIAL_STATE,
      open: true,
      total: instances.length,
      currentInstanceName: instances[0]?.name ?? '',
    });
  }, []);

  const close = useCallback(() => {
    cleanupListener();
    setState((prev) => ({ ...prev, open: false }));
  }, [cleanupListener]);

  const resetAfterClose = useCallback(() => {
    cleanupListener();
    setState(INITIAL_STATE);
  }, [cleanupListener]);

  const start = useCallback(async () => {
    const instances = instancesRef.current;
    const latestVersion = latestVersionRef.current;
    if (!latestVersion || instances.length === 0) {
      return;
    }

    setState((prev) => ({
      ...prev,
      currentIndex: 0,
      currentInstanceName: instances[0]?.name ?? '',
      step: null,
      progress: 0,
      error: null,
    }));

    try {
      unlistenRef.current = await listen<DeployProgress>('deploy-progress', (event) => {
        const { step, progress } = event.payload;
        setState((prev) => ({
          ...prev,
          step,
          progress,
        }));
      });

      const { versions } = useAppStore.getState();
      if (!versions.some((v) => v.version === latestVersion)) {
        const release = releases.find((r) => r.tag_name === latestVersion);
        if (!release) {
          throw new Error(`版本 ${latestVersion} 不存在`);
        }

        setState((prev) => ({
          ...prev,
          currentInstanceName: '下载版本',
          step: null,
          progress: 0,
        }));
        await api.installVersion(release);
      }

      for (let i = 0; i < instances.length; i++) {
        const instance = instances[i];
        setState((prev) => ({
          ...prev,
          currentIndex: i,
          currentInstanceName: instance.name,
          step: null,
          progress: 0,
        }));

        await api.updateInstance(
          instance.id,
          instance.name,
          latestVersion,
          instance.configured_host,
          instance.configured_port,
          instance.check_update_enabled
        );
      }

      setState((prev) => ({
        ...prev,
        currentIndex: instances.length,
        currentInstanceName: '',
        step: 'done',
        progress: 100,
      }));
      message.success('批量更新完成');
    } catch (error) {
      handleApiError(error);
      setState((prev) => ({
        ...prev,
        error: '更新过程中出现错误，请查看日志',
      }));
    } finally {
      cleanupListener();
    }
  }, [releases, cleanupListener]);

  return {
    state,
    open,
    start,
    close,
    resetAfterClose,
  };
}
