import type { IEditResubmitObservation } from '@/common/adapter/ipcBridge';
import type { ConversationId, MessageId } from '@/common/types/ids';

import type { EditResubmitRequestOutcome } from '../platforms/nomi/editResubmitRecovery';
import type { EditingMessagePhase } from './editingMessageStore';

export type EditResubmitOperationSource = 'edit' | 'retry';

export interface EditResubmitOperationRecord {
  conversationId: ConversationId;
  operationId: string;
  targetMessageId: MessageId;
  targetCreatedAt: number;
  originalContent: string;
  backendInput?: string;
  attachmentPaths: readonly string[];
  draftRevision: number;
  source: EditResubmitOperationSource;
  phase: EditingMessagePhase;
  requestOutcome?: EditResubmitRequestOutcome;
  lastObservation?: IEditResubmitObservation;
  runnerOwnerId?: string;
}

const operations = new Map<ConversationId, EditResubmitOperationRecord>();
const listeners = new Set<() => void>();

const emit = (): void => {
  for (const listener of listeners) listener();
};

/** Synchronous admission shared by every SendBox instance and entry point. */
export const beginEditResubmitOperation = (
  record: EditResubmitOperationRecord
): boolean => {
  if (operations.has(record.conversationId)) return false;
  operations.set(record.conversationId, { ...record });
  emit();
  return true;
};

export const getEditResubmitOperation = (
  conversationId: ConversationId
): EditResubmitOperationRecord | undefined => operations.get(conversationId);

export const updateEditResubmitOperation = (
  conversationId: ConversationId,
  operationId: string,
  patch: Partial<Omit<EditResubmitOperationRecord, 'conversationId' | 'operationId'>>
): boolean => {
  const current = operations.get(conversationId);
  if (!current || current.operationId !== operationId) return false;
  operations.set(conversationId, { ...current, ...patch });
  emit();
  return true;
};

export const releaseEditResubmitOperation = (
  conversationId: ConversationId,
  operationId: string
): boolean => {
  const current = operations.get(conversationId);
  if (!current || current.operationId !== operationId) return false;
  operations.delete(conversationId);
  emit();
  return true;
};

/** A runner lease moves across remounts without changing the operation key. */
export const claimEditResubmitRunner = (
  conversationId: ConversationId,
  operationId: string,
  ownerId: string
): boolean => {
  const current = operations.get(conversationId);
  if (!current || current.operationId !== operationId || current.runnerOwnerId) return false;
  operations.set(conversationId, { ...current, runnerOwnerId: ownerId });
  emit();
  return true;
};

export const releaseEditResubmitRunner = (
  conversationId: ConversationId,
  operationId: string,
  ownerId: string
): void => {
  const current = operations.get(conversationId);
  if (
    !current ||
    current.operationId !== operationId ||
    current.runnerOwnerId !== ownerId
  ) {
    return;
  }
  operations.set(conversationId, { ...current, runnerOwnerId: undefined });
  emit();
};

export const subscribeEditResubmitOperations = (listener: () => void): (() => void) => {
  listeners.add(listener);
  return () => listeners.delete(listener);
};

/**
 * Observe a submitted operation becoming runner-free. This closes the overlap
 * window where a remount can subscribe while the previous renderer still owns
 * the runner and must resume only after that owner releases it.
 */
export const subscribeRecoverableEditResubmitOperation = (
  conversationId: ConversationId,
  listener: (operation: EditResubmitOperationRecord) => void
): (() => void) => {
  let notifiedOperationId: string | undefined;
  let scheduledOperationId: string | undefined;
  let subscribed = true;
  const notifyIfAvailable = (): void => {
    const operation = operations.get(conversationId);
    if (
      !operation ||
      operation.phase === 'editing' ||
      operation.runnerOwnerId !== undefined
    ) {
      notifiedOperationId = undefined;
      scheduledOperationId = undefined;
      return;
    }
    if (
      notifiedOperationId === operation.operationId ||
      scheduledOperationId === operation.operationId
    ) return;
    const operationId = operation.operationId;
    scheduledOperationId = operationId;
    queueMicrotask(() => {
      if (!subscribed) return;
      if (scheduledOperationId !== operationId) return;
      scheduledOperationId = undefined;
      const current = operations.get(conversationId);
      if (
        !current ||
        current.operationId !== operationId ||
        current.phase === 'editing' ||
        current.runnerOwnerId !== undefined ||
        notifiedOperationId === operationId
      ) return;
      notifiedOperationId = operationId;
      listener(current);
    });
  };
  const unsubscribe = subscribeEditResubmitOperations(notifyIfAvailable);
  notifyIfAvailable();
  return () => {
    subscribed = false;
    unsubscribe();
  };
};

export const __resetEditResubmitOperations = (): void => {
  operations.clear();
  emit();
};
