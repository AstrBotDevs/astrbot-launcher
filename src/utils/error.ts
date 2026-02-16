import { message } from '../antdStatic';
import { getErrorText } from '../constants/errorCodes';
import type { AppError } from '../types';

function isAppError(error: unknown): error is AppError {
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

/**
 * Handle API error by showing a message notification.
 */
export function handleApiError(error: unknown, fallbackMessage = '操作失败'): void {
  const msg = getErrorMessage(error);
  message.error(msg || fallbackMessage);
}
