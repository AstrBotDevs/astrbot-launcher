import { useCallback } from 'react';
import { handleApiError } from '../utils';
import { useAppStore } from '../stores';

export const SKIP_OPERATION = Symbol('skip-operation');

type SkipOperation = typeof SKIP_OPERATION;

interface RunOperationOptions<T> {
  key: string;
  task: () => Promise<T | SkipOperation>;
  reloadBefore?: boolean;
  reloadAfter?: boolean;
  onSuccess?: (result: T) => void | Promise<void>;
  onError?: (error: unknown) => void | Promise<void>;
}

/**
 * Shared operation pipeline:
 * start loading state -> optional pre-reload -> task -> optional post-reload -> callbacks -> finish loading state
 */
export function useOperationRunner() {
  const startOperation = useAppStore((s) => s.startOperation);
  const finishOperation = useAppStore((s) => s.finishOperation);
  const reloadSnapshot = useAppStore((s) => s.reloadSnapshot);

  const runOperation = useCallback(
    async <T,>({
      key,
      task,
      reloadBefore = false,
      reloadAfter = true,
      onSuccess,
      onError,
    }: RunOperationOptions<T>): Promise<boolean> => {
      startOperation(key);
      try {
        if (reloadBefore) {
          await reloadSnapshot();
        }

        const result = await task();
        if (result === SKIP_OPERATION) {
          return false;
        }

        if (reloadAfter) {
          await reloadSnapshot({ throwOnError: true });
        }

        if (onSuccess) {
          await onSuccess(result);
        }
        return true;
      } catch (error) {
        if (onError) {
          await onError(error);
        } else {
          handleApiError(error);
        }
        return false;
      } finally {
        finishOperation(key);
      }
    },
    [startOperation, finishOperation, reloadSnapshot]
  );

  return { runOperation };
}
