import { useCallback, useRef, useState } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { api } from '../api';
import { message } from '../antdStatic';
import { useAppStore } from '../stores';
import { getErrorMessage, handleApiError, parseProcessLockingError } from '../utils';
import type { DeployProgress, GitHubRelease, InstanceStatus } from '../types';

export interface LockCheckItem {
  instance: InstanceStatus;
  detail: string;
  lockingProcesses: string[];
}

export interface BatchUpgradeState {
  open: boolean;
  phase: 'confirm' | 'lockCheck' | 'upgrading' | 'done' | 'error';
  total: number;
  currentIndex: number;
  currentInstanceName: string;
  step: string | null;
  progress: number;
  error: string | null;
  lockCheckItems: LockCheckItem[];
}

const INITIAL_STATE: BatchUpgradeState = {
  open: false,
  phase: 'confirm',
  total: 0,
  currentIndex: 0,
  currentInstanceName: '',
  step: null,
  progress: 0,
  error: null,
  lockCheckItems: [],
};

export function useBatchUpgrade(releases: GitHubRelease[]) {
  const [state, setState] = useState<BatchUpgradeState>(INITIAL_STATE);
  const unlistenRef = useRef<UnlistenFn | null>(null);
  const instancesRef = useRef<InstanceStatus[]>([]);
  const latestVersionRef = useRef<string | null>(null);
  const currentInstanceIdRef = useRef<string | null>(null);

  const cleanupListener = useCallback(() => {
    if (unlistenRef.current) {
      unlistenRef.current();
      unlistenRef.current = null;
    }
  }, []);

  const open = useCallback((instances: InstanceStatus[], latestVersion: string) => {
    instancesRef.current = instances;
    latestVersionRef.current = latestVersion;
    currentInstanceIdRef.current = null;
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

  const runUpgrade = useCallback(
    async (instances: InstanceStatus[]) => {
      const latestVersion = latestVersionRef.current;
      if (!latestVersion || instances.length === 0) {
        return;
      }

      setState((prev) => ({
        ...prev,
        phase: 'upgrading',
        currentIndex: 0,
        currentInstanceName: instances[0]?.name ?? '',
        step: null,
        progress: 0,
        error: null,
        lockCheckItems: [],
      }));

      try {
        unlistenRef.current = await listen<DeployProgress>('deploy-progress', (event) => {
          const { instance_id, step, progress } = event.payload;
          const currentId = currentInstanceIdRef.current;
          if (currentId && instance_id !== currentId) {
            return;
          }
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
          currentInstanceIdRef.current = null;
          await api.installVersion(release);
        }

        for (let i = 0; i < instances.length; i++) {
          const instance = instances[i];
          currentInstanceIdRef.current = instance.id;
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

        currentInstanceIdRef.current = null;
        setState((prev) => ({
          ...prev,
          phase: 'done',
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
          phase: 'error',
          error: getErrorMessage(error) || '更新过程中出现错误，请查看日志',
        }));
      } finally {
        cleanupListener();
      }
    },
    [releases, cleanupListener]
  );

  const start = useCallback(async () => {
    const instances = instancesRef.current;
    if (instances.length === 0) {
      return;
    }

    setState((prev) => ({
      ...prev,
      phase: 'lockCheck',
      currentInstanceName: '检查文件锁',
      step: null,
      progress: 0,
      error: null,
      lockCheckItems: [],
    }));

    const lockCheckItems: LockCheckItem[] = [];
    try {
      for (const instance of instances) {
        try {
          await api.checkLock({ target: 'instance_upgrade', instanceId: instance.id });
        } catch (error) {
          const lockError = parseProcessLockingError(error);
          if (!lockError) {
            throw error;
          }
          if (!lockError.canContinue) {
            throw new Error(`实例 ${instance.name} 被锁定：${lockError.detail}`);
          }
          lockCheckItems.push({
            instance,
            detail: lockError.detail,
            lockingProcesses: lockError.lockingProcesses,
          });
        }
      }

      if (lockCheckItems.length > 0) {
        setState((prev) => ({
          ...prev,
          phase: 'confirm',
          lockCheckItems,
        }));
        return;
      }

      await runUpgrade(instances);
    } catch (error) {
      handleApiError(error);
      setState((prev) => ({
        ...prev,
        phase: 'error',
        error: getErrorMessage(error) || '检查文件锁失败',
      }));
    }
  }, [runUpgrade]);

  const continueAfterLockCheck = useCallback(() => {
    void runUpgrade(instancesRef.current);
  }, [runUpgrade]);

  return {
    state,
    open,
    start,
    continueAfterLockCheck,
    close,
    resetAfterClose,
  };
}
