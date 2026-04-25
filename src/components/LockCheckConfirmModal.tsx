import { ConfirmModal } from './ConfirmModal';
import type { LockCheckModalState } from '../hooks/useLockCheckModal';

type LockCheckConfirmModalProps<TPayload> = {
  state: LockCheckModalState<TPayload> | null;
  loading?: boolean;
  onContinue: (payload: TPayload) => void | Promise<void>;
  onClose: () => void;
};

export function LockCheckConfirmModal<TPayload>({
  state,
  loading = false,
  onContinue,
  onClose,
}: LockCheckConfirmModalProps<TPayload>) {
  return (
    <ConfirmModal
      open={state !== null}
      title={state?.mode === 'locked' ? '目标路径被占用' : '目标路径占用检测失败'}
      danger={state?.mode === 'locked'}
      okText={state?.mode === 'checkFailed' ? '继续操作' : '我知道了'}
      loading={loading}
      content={
        <>
          <p>{state?.detail}</p>
          {state?.lockingProcesses?.length ? (
            <div>
              <p>检测到以下进程正在占用目标路径:</p>
              {state.lockingProcesses.map((process, index) => (
                <p key={`${process}-${index}`}>
                  {index + 1}. {process}
                </p>
              ))}
            </div>
          ) : null}
          {state?.mode === 'checkFailed' ? (
            <p>可选择继续执行本次操作，或取消后稍后重试。</p>
          ) : (
            <p>请关闭相关进程后重试。</p>
          )}
        </>
      }
      onConfirm={state?.mode === 'checkFailed' ? () => void onContinue(state.payload) : onClose}
      onCancel={onClose}
    />
  );
}
