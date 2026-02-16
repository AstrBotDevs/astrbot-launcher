import { useCallback } from 'react';
import { message } from '../antdStatic';
import { api } from '../api';
import type { InstanceStatus } from '../types';
import { handleApiError } from '../utils';
import { STATUS_MESSAGES, OPERATION_KEYS } from '../constants';
import { useAppStore } from '../stores';
import { SKIP_OPERATION, useOperationRunner } from './useOperationRunner';

/**
 * Hook for handling instance version upgrade flow.
 * The backend now handles the full pipeline: backup → deploy → restore → cleanup.
 */
export function useInstanceUpgrade() {
  const startDeploy = useAppStore((s) => s.startDeploy);
  const closeDeploy = useAppStore((s) => s.closeDeploy);
  const { runOperation } = useOperationRunner();

  const upgradeInstance = useCallback(
    async (instance: InstanceStatus, newName: string, newVersion: string): Promise<boolean> => {
      return runOperation({
        key: OPERATION_KEYS.instance(instance.id),
        reloadBefore: true,
        task: async () => {
          const { instances } = useAppStore.getState();
          const latestInstance = instances.find((i) => i.id === instance.id);
          if (!latestInstance) {
            message.warning('实例不存在或已被删除');
            closeDeploy();
            return SKIP_OPERATION;
          }

          const cmp = await api.compareVersions(newVersion, instance.version);
          const deployType = cmp > 0 ? 'upgrade' : 'downgrade';
          startDeploy(latestInstance.name, deployType);

          await api.updateInstance(instance.id, newName, newVersion);
        },
        onSuccess: () => {
          message.success(STATUS_MESSAGES.INSTANCE_UPDATED);
          // done event from backend auto-closes the modal via event listener
        },
        onError: (error) => {
          handleApiError(error);
          closeDeploy();
        },
      });
    },
    [startDeploy, closeDeploy, runOperation]
  );

  return { upgradeInstance };
}
