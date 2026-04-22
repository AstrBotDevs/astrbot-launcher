import { message } from '../antdStatic';
import { ErrorCode, getErrorText } from '../constants/errorCodes';
import type { AppError } from '../types';

export type LockCheckFailureReason = 'check_failed' | 'locked';

export interface ProcessLockingErrorInfo {
  reason: LockCheckFailureReason;
  detail: string;
  canContinue: boolean;
  lockingProcesses: string[];
}

export function isAppError(error: unknown): error is AppError {
  return (
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    typeof (error as AppError).code === 'number' &&
    'payload' in error &&
    typeof (error as AppError).payload === 'object'
  );
}

/**
 * Extract error message from unknown error type.
 */
export function getErrorMessage(error: unknown): string {
  if (isAppError(error)) {
    return getErrorText(error.code, error.payload);
  }
  if (error instanceof Error) return error.message;
  if (typeof error === 'string') return error;
  return String(error);
}

function parseLockingProcesses(rawValue: string | undefined): string[] {
  if (!rawValue) return [];
  try {
    const parsed = JSON.parse(rawValue);
    if (Array.isArray(parsed)) {
      return parsed.filter((item): item is string => typeof item === 'string');
    }
    return [];
  } catch {
    return [];
  }
}

export function parseProcessLockingError(error: unknown): ProcessLockingErrorInfo | null {
  if (!isAppError(error) || error.code !== ErrorCode.PROCESS_LOCKING) {
    return null;
  }

  const reason: LockCheckFailureReason =
    error.payload.reason === 'locked' ? 'locked' : 'check_failed';
  const detail = getErrorText(error.code, error.payload);

  return {
    reason,
    detail,
    canContinue: reason === 'check_failed' && error.payload.can_continue === 'true',
    lockingProcesses: parseLockingProcesses(error.payload.locking_processes),
  };
}

/**
 * Handle API error by showing a message notification.
 */
export function handleApiError(error: unknown, fallbackMessage = '操作失败'): void {
  const msg = getErrorMessage(error);
  message.error(msg || fallbackMessage);
}
