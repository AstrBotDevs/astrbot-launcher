import { useCallback, useState } from 'react';
import { parseProcessLockingError } from '../utils';

type LockCheckModalBase = {
  detail: string;
  lockingProcesses: string[];
};

export type LockCheckModalState<TPayload> =
  | ({ mode: 'checkFailed'; payload: TPayload } & LockCheckModalBase)
  | ({ mode: 'locked' } & LockCheckModalBase);

type HandleLockCheckErrorOptions<TPayload> = {
  checkFailedPayload: TPayload;
  onLockCheckError?: (mode: 'checkFailed' | 'locked') => void;
};

export function useLockCheckModal<TPayload>() {
  const [lockCheckModal, setLockCheckModal] = useState<LockCheckModalState<TPayload> | null>(null);

  const closeLockCheckModal = useCallback(() => {
    setLockCheckModal(null);
  }, []);

  const handleLockCheckError = useCallback(
    (error: unknown, options: HandleLockCheckErrorOptions<TPayload>): boolean => {
      const lockCheckError = parseProcessLockingError(error);
      if (!lockCheckError) {
        return false;
      }

      const mode = lockCheckError.canContinue ? 'checkFailed' : 'locked';
      options.onLockCheckError?.(mode);
      if (mode === 'checkFailed') {
        setLockCheckModal({
          mode,
          payload: options.checkFailedPayload,
          detail: lockCheckError.detail,
          lockingProcesses: lockCheckError.lockingProcesses,
        });
      } else {
        setLockCheckModal({
          mode,
          detail: lockCheckError.detail,
          lockingProcesses: lockCheckError.lockingProcesses,
        });
      }
      return true;
    },
    []
  );

  return {
    lockCheckModal,
    setLockCheckModal,
    closeLockCheckModal,
    handleLockCheckError,
  };
}
