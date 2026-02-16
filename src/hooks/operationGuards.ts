import { message } from '../antdStatic';
import { SKIP_OPERATION } from './useOperationRunner';

export function findLatestOrSkip<T>(
  items: T[],
  predicate: (item: T) => boolean,
  warningMsg: string
): T | typeof SKIP_OPERATION {
  const found = items.find(predicate);
  if (!found) {
    message.warning(warningMsg);
    return SKIP_OPERATION;
  }
  return found;
}
