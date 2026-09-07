import { ipcBridge } from '@/common';
import type { ConversationId } from '@/common/types/ids';
import { conversationTarget } from '@/common/types/ids';
import { sessionStorageKey } from '@/common/utils/browserStorageKey';
import { uuidv7 } from '@/common/utils';
import { getConversationOrNull } from '@/renderer/pages/conversation/utils/conversationCache';
import { getConversationRuntimeAuthority } from '@/renderer/pages/conversation/utils/conversationRuntime';
import { useAddEventListener } from '@/renderer/utils/emitter';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import useSWR from 'swr';
import {
  getCommandQueueReconcileDelayMs,
  IDLE_EXECUTION_GATE,
  isCommandQueueExecutionCurrent,
  reduceCommandQueueExecutionGate,
  shouldDispatchConversationCommandQueue,
  type CommandQueueExecutionGate,
} from './commandQueueExecutionGate';
import { promoteQueuedCommand, reorderQueuedCommand } from './commandQueueItems';
import { isAuthoritativeCompletionRuntimeIdle } from './authoritativeTurnLifecyclePolicy';
import type { PublicMessageDeliveryDisposition } from './publicMessageDelivery';
import { classifyConversationSendFailure } from './conversationSendRecovery';

export { promoteQueuedCommand, reorderQueuedCommand };

export {
  reduceCommandQueueExecutionGate,
  type CommandQueueExecutionGate,
  type CommandQueueExecutionGateEvent,
} from './commandQueueExecutionGate';

export type ConversationCommandQueueItem = {
  id: string;
  input: string;
  files: string[];
  created_at: number;
  /** Persisted recovery metadata; omitted by pre-recovery queue entries. */
  recovery?: ConversationCommandQueueRecovery;
  /** Persisted delivery phase used to fail closed around ambiguous POSTs. */
  delivery_state?: ConversationCommandQueueDeliveryState;
};

export type ConversationCommandQueueRecovery = {
  admission_attempts: number;
  transport_attempts: number;
};

export type ConversationCommandQueueDeliveryState =
  | 'queued'
  | 'dispatching'
  | 'waiting_for_turn'
  | 'retrying'
  | 'paused';

export type ConversationCommandQueueState = {
  items: ConversationCommandQueueItem[];
  isPaused: boolean;
};

export const MAX_QUEUED_COMMANDS = 20;
export const MAX_QUEUED_COMMAND_INPUT_LENGTH = 20_000;
export const MAX_QUEUED_COMMAND_FILES = 50;
export const MAX_QUEUED_COMMAND_STATE_BYTES = 256 * 1024;
export const COMMAND_QUEUE_RUNTIME_QUERY_TIMEOUT_MS = 3_000;
export const MAX_ADMISSION_RECOVERY_ATTEMPTS = 1;
export const MAX_TRANSPORT_RECOVERY_ATTEMPTS = 3;

const getConversationForCommandQueue = async (conversationId: ConversationId) => {
  let timeout: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      getConversationOrNull(conversationId),
      new Promise<never>((_, reject) => {
        timeout = setTimeout(
          () => reject(new Error('Command queue runtime reconciliation timed out')),
          COMMAND_QUEUE_RUNTIME_QUERY_TIMEOUT_MS
        );
      }),
    ]);
  } finally {
    if (timeout) clearTimeout(timeout);
  }
};

export type QueueValidationFailureReason =
  | 'emptyInput'
  | 'inputTooLong'
  | 'tooManyFiles'
  | 'queueFull'
  | 'queueTooLarge';

type QueueValidationSuccess = {
  ok: true;
  nextStateBytes: number;
};

type QueueValidationFailure = {
  ok: false;
  reason: QueueValidationFailureReason;
};

const COMMAND_QUEUE_LOG_PREFIX = '[conversation-command-queue]';

const summarizeQueuedCommand = (item: ConversationCommandQueueItem): Record<string, unknown> => ({
  id: item.id,
  created_at: item.created_at,
  inputLength: item.input.length,
  fileCount: item.files.length,
  deliveryState: getQueueItemDeliveryState(item),
  recovery: getQueueItemRecovery(item),
  preview: item.input.replace(/\s+/g, ' ').trim().slice(0, 120),
});

const logCommandQueue = (conversation_id: ConversationId, event: string, payload: Record<string, unknown> = {}): void => {
  console.info(COMMAND_QUEUE_LOG_PREFIX, {
    conversation_id,
    event,
    ...payload,
  });
};

const createDefaultQueueState = (): ConversationCommandQueueState => ({
  items: [],
  isPaused: false,
});

const queueStore = new Map<ConversationId, ConversationCommandQueueState>();

const getStorageKey = (conversation_id: ConversationId): string =>
  sessionStorageKey('command-queue', conversationTarget(conversation_id));
const measureQueueStateBytes = (state: ConversationCommandQueueState): number =>
  new TextEncoder().encode(JSON.stringify(state)).length;

const uniqueFiles = (files: string[]): string[] => Array.from(new Set(files.filter(Boolean)));
const isInputEmpty = (input: string): boolean => input.trim().length === 0;

const DEFAULT_QUEUE_RECOVERY: ConversationCommandQueueRecovery = {
  admission_attempts: 0,
  transport_attempts: 0,
};

const normalizeRecovery = (value: unknown): ConversationCommandQueueRecovery => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return { ...DEFAULT_QUEUE_RECOVERY };
  }

  const candidate = value as Record<string, unknown>;
  const normalizeAttempts = (attempts: unknown): number =>
    typeof attempts === 'number' && Number.isFinite(attempts)
      ? Math.min(100, Math.max(0, Math.floor(attempts)))
      : 0;

  return {
    admission_attempts: normalizeAttempts(candidate.admission_attempts),
    transport_attempts: normalizeAttempts(candidate.transport_attempts),
  };
};

export const getQueueItemRecovery = (
  item: ConversationCommandQueueItem
): ConversationCommandQueueRecovery => normalizeRecovery(item.recovery);

export const getQueueItemDeliveryState = (
  item: ConversationCommandQueueItem
): ConversationCommandQueueDeliveryState => {
  switch (item.delivery_state) {
    case 'dispatching':
    case 'waiting_for_turn':
    case 'retrying':
    case 'paused':
    case 'queued':
      return item.delivery_state;
    default:
      return 'queued';
  }
};

export const isQueueItemDeliveryInFlight = (item: ConversationCommandQueueItem): boolean =>
  getQueueItemDeliveryState(item) === 'dispatching' || getQueueItemDeliveryState(item) === 'retrying';

const resetQueueItemRecovery = (
  item: ConversationCommandQueueItem,
  delivery_state: ConversationCommandQueueDeliveryState = 'queued'
): ConversationCommandQueueItem => ({
  ...item,
  recovery: { ...DEFAULT_QUEUE_RECOVERY },
  delivery_state,
});

const updateQueueItemDelivery = (
  item: ConversationCommandQueueItem,
  delivery_state: ConversationCommandQueueDeliveryState,
  recovery: Partial<ConversationCommandQueueRecovery> = {}
): ConversationCommandQueueItem => {
  const currentRecovery = getQueueItemRecovery(item);
  return {
    ...item,
    recovery: {
      ...currentRecovery,
      ...recovery,
    },
    delivery_state,
  };
};

const normalizeQueueItem = (item: unknown): ConversationCommandQueueItem | null => {
  if (!item || typeof item !== 'object') {
    return null;
  }

  const candidate = item as Record<string, unknown>;
  if (
    typeof candidate.id !== 'string' ||
    typeof candidate.input !== 'string' ||
    !Array.isArray(candidate.files) ||
    !candidate.files.every((file) => typeof file === 'string') ||
    typeof candidate.created_at !== 'number' ||
    !Number.isFinite(candidate.created_at)
  ) {
    return null;
  }

  const normalizedItem: ConversationCommandQueueItem = {
    id: candidate.id,
    input: candidate.input,
    files: uniqueFiles(candidate.files),
    created_at: candidate.created_at,
    recovery: normalizeRecovery(candidate.recovery),
    delivery_state: getQueueItemDeliveryState(candidate as ConversationCommandQueueItem),
  };

  if (
    isInputEmpty(normalizedItem.input) ||
    normalizedItem.input.length > MAX_QUEUED_COMMAND_INPUT_LENGTH ||
    normalizedItem.files.length > MAX_QUEUED_COMMAND_FILES
  ) {
    return null;
  }

  return normalizedItem;
};

export const normalizeQueueState = (state: unknown): ConversationCommandQueueState => {
  if (!state || typeof state !== 'object') {
    return createDefaultQueueState();
  }

  const candidate = state as Partial<ConversationCommandQueueState>;
  const normalizedItems = Array.isArray(candidate.items)
    ? candidate.items.map(normalizeQueueItem).filter((item): item is ConversationCommandQueueItem => item !== null)
    : [];
  const items: ConversationCommandQueueItem[] = [];

  for (const item of normalizedItems.slice(0, MAX_QUEUED_COMMANDS)) {
    const nextItems = [...items, item];
    const nextState = {
      items: nextItems,
      isPaused: Boolean(candidate.isPaused),
    };

    if (measureQueueStateBytes(nextState) > MAX_QUEUED_COMMAND_STATE_BYTES) {
      break;
    }

    items.push(item);
  }

  return {
    items,
    isPaused: items.length > 0 ? Boolean(candidate.isPaused) : false,
  };
};

export const createQueuedCommandItem = ({
  input,
  files,
}: Pick<ConversationCommandQueueItem, 'input' | 'files'>): ConversationCommandQueueItem => ({
  // This identifier is also the durable HTTP idempotency key. It must survive
  // dequeue restoration, remounts, and accepted-response loss unchanged.
  id: uuidv7(),
  input,
  files: uniqueFiles(files),
  created_at: Date.now(),
  recovery: { ...DEFAULT_QUEUE_RECOVERY },
  delivery_state: 'queued',
});

const getQueueValidationFailureReason = (state: ConversationCommandQueueState): QueueValidationFailureReason | null => {
  if (state.items.length > MAX_QUEUED_COMMANDS) {
    return 'queueFull';
  }

  if (state.items.some((item) => isInputEmpty(item.input))) {
    return 'emptyInput';
  }

  if (state.items.some((item) => item.input.length > MAX_QUEUED_COMMAND_INPUT_LENGTH)) {
    return 'inputTooLong';
  }

  if (state.items.some((item) => item.files.length > MAX_QUEUED_COMMAND_FILES)) {
    return 'tooManyFiles';
  }

  if (measureQueueStateBytes(state) > MAX_QUEUED_COMMAND_STATE_BYTES) {
    return 'queueTooLarge';
  }

  return null;
};

export const validateQueuedCommandItem = (
  item: ConversationCommandQueueItem,
  state: ConversationCommandQueueState
): QueueValidationSuccess | QueueValidationFailure => {
  const nextState = {
    ...state,
    items: [...state.items, item],
  };
  const failureReason = getQueueValidationFailureReason(nextState);
  if (failureReason) {
    return { ok: false, reason: failureReason };
  }
  const nextStateBytes = measureQueueStateBytes(nextState);
  return { ok: true, nextStateBytes };
};

const isQueueValidationFailure = (
  validation: QueueValidationSuccess | QueueValidationFailure
): validation is QueueValidationFailure => !validation.ok;

const readPersistedQueueState = (conversation_id: ConversationId): ConversationCommandQueueState => {
  if (queueStore.has(conversation_id)) {
    return queueStore.get(conversation_id) ?? createDefaultQueueState();
  }

  if (typeof window === 'undefined') {
    return createDefaultQueueState();
  }

  try {
    const stored = window.sessionStorage.getItem(getStorageKey(conversation_id));
    if (!stored) {
      return createDefaultQueueState();
    }

    const parsed = JSON.parse(stored) as unknown;
    const normalized = normalizeQueueState(parsed);
    queueStore.set(conversation_id, normalized);
    logCommandQueue(conversation_id, 'restored', {
      itemCount: normalized.items.length,
      isPaused: normalized.isPaused,
    });
    return normalized;
  } catch (error) {
    console.warn('[conversation-command-queue] Failed to read persisted queue state:', error);
    return createDefaultQueueState();
  }
};

const removePersistedQueueState = (conversation_id: ConversationId): boolean => {
  queueStore.delete(conversation_id);
  if (typeof window !== 'undefined') {
    try {
      window.sessionStorage.removeItem(getStorageKey(conversation_id));
    } catch (error) {
      console.warn('[conversation-command-queue] Failed to remove persisted queue state:', error);
      return false;
    }
  }
  return true;
};

const persistQueueState = (conversation_id: ConversationId, state: ConversationCommandQueueState): boolean => {
  const normalized = normalizeQueueState(state);

  if (normalized.items.length === 0 && !normalized.isPaused) {
    return removePersistedQueueState(conversation_id);
  }

  if (typeof window !== 'undefined') {
    try {
      window.sessionStorage.setItem(getStorageKey(conversation_id), JSON.stringify(normalized));
    } catch (error) {
      console.warn('[conversation-command-queue] Failed to persist queue state:', error);
      return false;
    }
  }
  queueStore.set(conversation_id, normalized);
  return true;
};

export const removeQueuedCommand = (
  items: ConversationCommandQueueItem[],
  commandId: string
): ConversationCommandQueueItem[] => items.filter((item) => item.id !== commandId);

export const restoreQueuedCommand = (
  items: ConversationCommandQueueItem[],
  failedItem: ConversationCommandQueueItem
): ConversationCommandQueueItem[] => [failedItem, ...removeQueuedCommand(items, failedItem.id)];

export const updateQueuedCommand = (
  items: ConversationCommandQueueItem[],
  commandId: string,
  updates: Partial<Pick<ConversationCommandQueueItem, 'input' | 'files'>>
): ConversationCommandQueueItem[] =>
  items.map((item) =>
    item.id === commandId
      ? {
          // Editing is a new user submission. Never mutate the payload behind
          // an id that may already have crossed the durable admission boundary.
          ...item,
          id: uuidv7(),
          created_at: Date.now(),
          ...updates,
          files: updates.files ? uniqueFiles(updates.files) : item.files,
          recovery: { ...DEFAULT_QUEUE_RECOVERY },
          delivery_state: 'queued',
        }
      : item
  );

export const shouldEnqueueConversationCommand = ({
  enabled = true,
  isBusy,
  hasPendingCommands,
}: {
  enabled?: boolean;
  isBusy: boolean;
  hasPendingCommands: boolean;
}): boolean => enabled && (isBusy || hasPendingCommands);

type UseConversationCommandQueueOptions = {
  conversation_id: ConversationId;
  enabled?: boolean;
  isBusy: boolean;
  isHydrated?: boolean;
  onExecute: (
    item: ConversationCommandQueueItem,
    execution?: ConversationCommandQueueExecution
  ) => Promise<PublicMessageDeliveryDisposition | void>;
};

export type ConversationCommandQueueExecution = {
  isCurrent: () => boolean;
};

type EnqueueCommandInput = Pick<ConversationCommandQueueItem, 'input' | 'files'>;
type UpdateCommandInput = Pick<ConversationCommandQueueItem, 'input'>;

const getQueueValidationMessage = (
  t: (key: string, options?: Record<string, unknown>) => string,
  reason: QueueValidationFailureReason
): string => {
  const warningKeyMap = {
    emptyInput: 'conversation.commandQueue.emptyInput',
    queueFull: 'conversation.commandQueue.queueFull',
    inputTooLong: 'conversation.commandQueue.inputTooLong',
    tooManyFiles: 'conversation.commandQueue.tooManyFiles',
    queueTooLarge: 'conversation.commandQueue.queueTooLarge',
  } as const;
  const defaultValueMap = {
    emptyInput: 'Queued commands cannot be empty.',
    queueFull: 'Queue is full. Remove a command before adding more.',
    inputTooLong: 'This queued command is too long. Shorten it before sending.',
    tooManyFiles: 'Too many files are attached to this queued command.',
    queueTooLarge: 'Queue data is too large to persist safely. Remove some queued commands first.',
  } as const;

  return t(warningKeyMap[reason], {
    count: MAX_QUEUED_COMMANDS,
    files: MAX_QUEUED_COMMAND_FILES,
    defaultValue: defaultValueMap[reason],
  });
};

export const useConversationCommandQueue = ({
  conversation_id,
  enabled = true,
  isBusy,
  isHydrated = true,
  onExecute,
}: UseConversationCommandQueueOptions) => {
  const { t } = useTranslation();
  // Internal persistence/logging is keyed by the canonical conversation ID
  // (SWR key, sessionStorage key, and queueStore Map key).
  const conversationKey = conversation_id;
  const { data = createDefaultQueueState(), mutate } = useSWR(
    [`/conversation-command-queue/${conversationKey}`, conversationKey, enabled],
    ([, id, is_enabled]) => (is_enabled ? readPersistedQueueState(id) : createDefaultQueueState())
  );

  const stateRef = useRef(data);
  const pausedRef = useRef(data.isPaused);
  const executionGateRef = useRef<CommandQueueExecutionGate>(IDLE_EXECUTION_GATE);
  const executionGenerationRef = useRef(0);
  const executionConversationKeyRef = useRef(conversationKey);
  const mountedRef = useRef(true);
  const interactionLockedRef = useRef(false);
  const [isInteractionLocked, setIsInteractionLocked] = useState(false);
  const [executionGateVersion, setExecutionGateVersion] = useState(0);

  const publishState = useCallback(
    (nextState: ConversationCommandQueueState): Promise<ConversationCommandQueueState | undefined> => {
      const normalized = normalizeQueueState(nextState);
      stateRef.current = normalized;
      pausedRef.current = normalized.isPaused;
      return mutate(normalized, { revalidate: false });
    },
    [mutate]
  );

  const persistStateOrWarn = useCallback(
    (nextState: ConversationCommandQueueState): ConversationCommandQueueState | undefined => {
      const normalized = normalizeQueueState(nextState);
      if (persistQueueState(conversationKey, normalized)) {
        return normalized;
      }
      Message.warning(
        t('conversation.commandQueue.persistenceFailed', {
          defaultValue: 'The message was kept in the composer because the queue could not be saved. Try again.',
        })
      );
      return undefined;
    },
    [conversationKey, t]
  );

  // Update during render so a promise owned by the previous conversation is
  // stale before passive effect cleanup has a chance to run.
  executionConversationKeyRef.current = conversationKey;

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      executionGenerationRef.current += 1;
    };
  }, []);

  useEffect(() => {
    executionGenerationRef.current += 1;
    executionGateRef.current = IDLE_EXECUTION_GATE;
    pausedRef.current = false;
    interactionLockedRef.current = false;
    setIsInteractionLocked(false);
    setExecutionGateVersion((version) => version + 1);

    return () => {
      executionGenerationRef.current += 1;
    };
  }, [conversationKey]);

  const reconcileActiveExecution = useCallback(async (): Promise<boolean> => {
    const gate = executionGateRef.current;
    if (gate.phase === 'idle') return true;

    try {
      const conversation = await getConversationForCommandQueue(conversationKey);
      if (executionGateRef.current !== gate) return false;
      const runtimeAuthority = getConversationRuntimeAuthority(conversation);
      if (runtimeAuthority === 'unknown') return false;
      const isProcessing = runtimeAuthority === 'processing';
      // If turn.started was lost, the accepted-send gate remains in
      // waiting_start. A later authoritative idle read must reconcile that
      // phase as a start acknowledgement; treating it as completion would
      // intentionally leave the gate closed forever.
      const purpose = gate.phase === 'waiting_start' ? 'start' : 'completion';
      const nextGate = reduceCommandQueueExecutionGate(gate, {
        type: 'runtimeReconciled',
        purpose,
        runtimeIsProcessing: isProcessing,
      });
      if (nextGate === gate) return false;
      executionGateRef.current = nextGate;
      logCommandQueue(conversationKey, 'execution-reconciled', {
        purpose,
        runtimeIsProcessing: isProcessing,
        nextPhase: nextGate.phase,
        pendingItemCount: stateRef.current.items.length,
      });
      setExecutionGateVersion((version) => version + 1);
      return true;
    } catch (error) {
      console.warn('[conversation-command-queue] Failed to reconcile active execution:', error);
      return false;
    }
  }, [conversationKey]);

  useEffect(() => {
    stateRef.current = data;
  }, [data]);

  useEffect(() => {
    let disposed = false;

    const releaseExecutionGate = (expectedGate: CommandQueueExecutionGate, source: 'correlated' | 'runtime') => {
      if (disposed || executionGateRef.current !== expectedGate) return;
      executionGateRef.current = IDLE_EXECUTION_GATE;
      logCommandQueue(conversationKey, 'turn-completed', {
        source,
        pendingItemCount: stateRef.current.items.length,
      });
      setExecutionGateVersion((version) => version + 1);
    };

    const unsubscribeStarted = ipcBridge.conversation.turnStarted.on((event) => {
      if (event.conversation_id !== conversationKey) return;

      const previousGate = executionGateRef.current;
      const nextGate = reduceCommandQueueExecutionGate(previousGate, {
        type: 'turnStarted',
        turnId: event.turn_id,
      });
      if (nextGate === previousGate) return;

      executionGateRef.current = nextGate;
      logCommandQueue(conversationKey, 'turn-started', {
        turnId: event.turn_id,
        pendingItemCount: stateRef.current.items.length,
      });
      setExecutionGateVersion((version) => version + 1);
    });

    const unsubscribeCompleted = ipcBridge.conversation.turnCompleted.on((event) => {
      if (
        event.conversation_id !== conversationKey ||
        !isAuthoritativeCompletionRuntimeIdle(event.runtime)
      ) {
        return;
      }

      const gate = executionGateRef.current;
      if (gate.phase === 'idle') return;

      // A completion racing a newly submitted command still waiting for its
      // start acknowledgement may belong to the previous generation. Only the
      // send promise's accepted-result reconciliation may advance this phase.
      if (gate.phase === 'waiting_start') return;

      // Correlated completions carry the exact generation id. Never let a
      // delayed completion from an older turn release the gate for a newer turn.
      const completedGate = reduceCommandQueueExecutionGate(gate, {
        type: 'turnCompleted',
        turnId: event.turn_id,
        runtimeIsProcessing: event.runtime.is_processing,
      });
      if (completedGate.phase === 'idle') {
        releaseExecutionGate(gate, 'correlated');
        return;
      }

      // A mismatched correlated completion belongs to an older generation and
      // must never be upgraded into an uncorrelated runtime release.
      if (gate.phase === 'waiting_completion' && gate.turnId && event.turn_id) return;

      // A current stop/idle completion may intentionally omit turn_id. Re-read
      // the authoritative runtime after the event. The identity check in
      // releaseExecutionGate also rejects a start/reset that races this request.
      void reconcileActiveExecution();
    });

    return () => {
      disposed = true;
      unsubscribeStarted();
      unsubscribeCompleted();
    };
  }, [conversationKey, reconcileActiveExecution]);

  useEffect(() => {
    pausedRef.current = data.isPaused;
  }, [data.isPaused]);

  // A manually-started turn (or a running conversation restored on mount) also
  // owns the queue gate. Queue items added during that turn must wait for the
  // authoritative completion event, not the earlier visual spinner down-edge.
  useEffect(() => {
    if (!isBusy || executionGateRef.current.phase !== 'idle') return;
    executionGateRef.current = { phase: 'waiting_completion' };
    setExecutionGateVersion((version) => version + 1);
  }, [isBusy]);

  // A persisted admission recovery has already observed a lifecycle conflict.
  // On remount, restore its authoritative wait fence before dispatching; an
  // old `waiting_for_turn` item must not immediately create another 409.
  useEffect(() => {
    if (
      !enabled ||
      isBusy ||
      executionGateRef.current.phase !== 'idle' ||
      !data.items.some((item) => getQueueItemDeliveryState(item) === 'waiting_for_turn')
    ) {
      return;
    }
    executionGateRef.current = { phase: 'waiting_completion' };
    logCommandQueue(conversationKey, 'restored-admission-recovery-fence', {
      pendingItemCount: data.items.length,
    });
    setExecutionGateVersion((version) => version + 1);
  }, [conversationKey, data.items, enabled, isBusy]);

  // A missing turn.completed event must not strand the queue forever. The
  // visual busy down-edge only schedules reconciliation; the gate is released
  // after GET confirms the backend runtime is no longer processing. Failed or
  // timed-out reads keep retrying with a capped backoff for as long as this
  // idle UI still owns a non-idle gate, so service recovery always converges.
  useEffect(() => {
    if (isBusy || executionGateRef.current.phase === 'idle') return;
    let cancelled = false;

    void (async () => {
      let attempt = 0;
      while (!cancelled) {
        const delayMs = getCommandQueueReconcileDelayMs(attempt);
        await new Promise<void>((resolve) => setTimeout(resolve, delayMs));
        if (cancelled) return;
        if (await reconcileActiveExecution()) return;
        attempt += 1;
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [executionGateVersion, isBusy, reconcileActiveExecution]);

  useEffect(() => {
    interactionLockedRef.current = isInteractionLocked;
  }, [isInteractionLocked]);

  useEffect(() => {
    if (enabled) {
      return;
    }

    executionGateRef.current = IDLE_EXECUTION_GATE;
    executionGenerationRef.current += 1;
    pausedRef.current = false;
    interactionLockedRef.current = false;
    stateRef.current = createDefaultQueueState();
    setIsInteractionLocked(false);
    removePersistedQueueState(conversationKey);
    void mutate(createDefaultQueueState(), { revalidate: false });
  }, [conversation_id, enabled, mutate]);

  const updateState = useCallback(
    (
      updater: (state: ConversationCommandQueueState) => ConversationCommandQueueState
    ): Promise<ConversationCommandQueueState | undefined> => {
      if (!enabled) {
        const nextState = createDefaultQueueState();
        stateRef.current = nextState;
        pausedRef.current = false;
        removePersistedQueueState(conversationKey);
        return Promise.resolve(nextState);
      }

      const currentState = normalizeQueueState(stateRef.current);
      const nextState = normalizeQueueState(updater(currentState));
      const persistedState = persistStateOrWarn(nextState);
      return persistedState ? publishState(persistedState) : Promise.resolve(undefined);
    },
    [conversation_id, enabled, persistStateOrWarn, publishState]
  );

  const clear = useCallback(() => {
    const currentState = normalizeQueueState(stateRef.current);
    if (currentState.items.some(isQueueItemDeliveryInFlight)) {
      logCommandQueue(conversationKey, 'clear-rejected', {
        reason: 'delivery-in-progress',
        itemCount: currentState.items.length,
      });
      Message.warning(
        t('conversation.commandQueue.deliveryInProgress', {
          defaultValue: 'This message is still being sent. Wait for the result before clearing the queue.',
        })
      );
      return false;
    }

    pausedRef.current = false;
    logCommandQueue(conversationKey, 'cleared');
    void updateState(() => createDefaultQueueState());
    return true;
  }, [conversation_id, conversationKey, t, updateState]);

  useAddEventListener(
    'conversation.deleted',
    (deletedConversationId) => {
      if (deletedConversationId !== conversationKey) {
        return;
      }
      executionGenerationRef.current += 1;
      executionGateRef.current = IDLE_EXECUTION_GATE;
      const deletedState = createDefaultQueueState();
      stateRef.current = deletedState;
      pausedRef.current = false;
      void mutate(deletedState, { revalidate: false });
      removePersistedQueueState(conversationKey);
    },
    [conversationKey, conversation_id, mutate]
  );

  const enqueue = useCallback(
    ({ input, files }: EnqueueCommandInput) => {
      if (!enabled) {
        return null;
      }

      const currentState = normalizeQueueState(stateRef.current);
      const item = createQueuedCommandItem({ input, files });
      const validation = validateQueuedCommandItem(item, currentState);

      if (isQueueValidationFailure(validation)) {
        const reason: QueueValidationFailureReason = validation.reason;
        logCommandQueue(conversationKey, 'enqueue-rejected', {
          reason,
          item: summarizeQueuedCommand(item),
          currentItemCount: currentState.items.length,
        });
        Message.warning(getQueueValidationMessage(t, reason));
        return null;
      }

      const nextState: ConversationCommandQueueState = {
        ...currentState,
        items: [...currentState.items, item],
      };
      const persistedState = persistStateOrWarn(nextState);
      if (!persistedState) {
        logCommandQueue(conversationKey, 'enqueue-rejected', {
          reason: 'persistence-failed',
          item: summarizeQueuedCommand(item),
          currentItemCount: currentState.items.length,
        });
        return null;
      }

      logCommandQueue(conversationKey, 'enqueued', {
        item: summarizeQueuedCommand(item),
        currentItemCount: currentState.items.length,
      });
      void publishState(persistedState);
      return item;
    },
    [conversation_id, conversationKey, enabled, persistStateOrWarn, publishState, t]
  );

  const update = useCallback(
    (commandId: string, { input }: UpdateCommandInput) => {
      if (!enabled) {
        return false;
      }

      const currentState = normalizeQueueState(stateRef.current);
      const currentItem = currentState.items.find((item) => item.id === commandId);
      if (!currentItem) {
        return false;
      }

      if (isQueueItemDeliveryInFlight(currentItem)) {
        logCommandQueue(conversationKey, 'update-rejected', {
          reason: 'delivery-in-progress',
          commandId,
        });
        Message.warning(
          t('conversation.commandQueue.deliveryInProgress', {
            defaultValue: 'This message is still being sent. Wait for the result before editing it.',
          })
        );
        return false;
      }

      const nextItems = updateQueuedCommand(currentState.items, commandId, { input });
      const nextState: ConversationCommandQueueState = {
        isPaused: false,
        items: nextItems,
      };
      const failureReason = getQueueValidationFailureReason(nextState);

      if (failureReason) {
        logCommandQueue(conversationKey, 'update-rejected', {
          reason: failureReason,
          commandId,
          inputLength: input.length,
        });
        Message.warning(getQueueValidationMessage(t, failureReason));
        return false;
      }

      const persistedState = persistStateOrWarn(nextState);
      if (!persistedState) {
        logCommandQueue(conversationKey, 'update-rejected', {
          reason: 'persistence-failed',
          commandId,
          inputLength: input.length,
        });
        return false;
      }

      logCommandQueue(conversationKey, 'updated', {
        commandId,
        inputLength: input.length,
      });
      void publishState(persistedState);
      return true;
    },
    [conversation_id, conversationKey, enabled, persistStateOrWarn, publishState, t]
  );

  const remove = useCallback(
    (commandId: string) => {
      if (!enabled) {
        return false;
      }

      const currentState = normalizeQueueState(stateRef.current);
      const currentItem = currentState.items.find((item) => item.id === commandId);
      if (!currentItem) {
        return false;
      }

      if (isQueueItemDeliveryInFlight(currentItem)) {
        logCommandQueue(conversationKey, 'remove-rejected', {
          reason: 'delivery-in-progress',
          commandId,
        });
        Message.warning(
          t('conversation.commandQueue.deliveryInProgress', {
            defaultValue: 'This message is still being sent. Wait for the result before removing it.',
          })
        );
        return false;
      }

      const nextState: ConversationCommandQueueState = {
        items: removeQueuedCommand(currentState.items, commandId),
        isPaused: false,
      };
      const persistedState = persistStateOrWarn(nextState);
      if (!persistedState) {
        logCommandQueue(conversationKey, 'remove-rejected', {
          reason: 'persistence-failed',
          commandId,
        });
        return false;
      }

      logCommandQueue(conversationKey, 'removed', {
        commandId,
      });
      void publishState(persistedState);
      return true;
    },
    [conversation_id, conversationKey, enabled, persistStateOrWarn, publishState, t]
  );

  const reorder = useCallback(
    (activeCommandId: string, overCommandId: string) => {
      if (!enabled) {
        return false;
      }

      const currentState = normalizeQueueState(stateRef.current);
      if (currentState.items.some(isQueueItemDeliveryInFlight)) {
        logCommandQueue(conversationKey, 'reorder-rejected', {
          reason: 'delivery-in-progress',
          activeCommandId,
          overCommandId,
        });
        Message.warning(
          t('conversation.commandQueue.deliveryInProgress', {
            defaultValue: 'A message is still being sent. Wait for the result before reordering the queue.',
          })
        );
        return false;
      }

      const nextState: ConversationCommandQueueState = {
        isPaused: false,
        items: reorderQueuedCommand(currentState.items, activeCommandId, overCommandId),
      };
      const persistedState = persistStateOrWarn(nextState);
      if (!persistedState) {
        logCommandQueue(conversationKey, 'reorder-rejected', {
          reason: 'persistence-failed',
          activeCommandId,
          overCommandId,
        });
        return false;
      }

      logCommandQueue(conversationKey, 'reordered', {
        activeCommandId,
        overCommandId,
      });
      void publishState(persistedState);
      return true;
    },
    [conversation_id, conversationKey, enabled, persistStateOrWarn, publishState, t]
  );

  const sendNow = useCallback(
    (commandId: string) => {
      if (!enabled) {
        return false;
      }

      const currentState = normalizeQueueState(stateRef.current);
      const currentItem = currentState.items.find((item) => item.id === commandId);
      if (!currentItem) {
        return false;
      }

      if (isQueueItemDeliveryInFlight(currentItem)) {
        logCommandQueue(conversationKey, 'send-now-rejected', {
          reason: 'delivery-in-progress',
          commandId,
        });
        Message.warning(
          t('conversation.commandQueue.deliveryInProgress', {
            defaultValue: 'This message is still being sent. Wait for the result before moving it.',
          })
        );
        return false;
      }

      const nextState: ConversationCommandQueueState = {
        isPaused: currentState.items.length > 0 ? false : currentState.isPaused,
        items: promoteQueuedCommand(currentState.items, commandId),
      };
      const persistedState = persistStateOrWarn(nextState);
      if (!persistedState) {
        logCommandQueue(conversationKey, 'send-now-rejected', {
          reason: 'persistence-failed',
          commandId,
        });
        return false;
      }

      pausedRef.current = false;
      logCommandQueue(conversationKey, 'send-now', {
        commandId,
      });
      void publishState(persistedState);
      return true;
    },
    [conversation_id, conversationKey, enabled, persistStateOrWarn, publishState, t]
  );

  const pause = useCallback(() => {
    if (!enabled) {
      return;
    }

    pausedRef.current = true;
    logCommandQueue(conversationKey, 'paused', {
      itemCount: data.items.length,
    });
    void updateState((state) => {
      if (state.items.length === 0) {
        pausedRef.current = false;
        return createDefaultQueueState();
      }
      return {
        ...state,
        isPaused: true,
      };
    });
  }, [conversation_id, data.items.length, enabled, updateState]);

  const resume = useCallback(() => {
    if (!enabled) {
      return;
    }

    pausedRef.current = false;
    logCommandQueue(conversationKey, 'resumed', {
      itemCount: data.items.length,
    });
    void updateState((state) => ({
      ...state,
      items: state.items.map((item) =>
        isQueueItemDeliveryInFlight(item)
          ? item
          : resetQueueItemRecovery(item, 'queued')
      ),
      isPaused: state.items.length > 0 ? false : state.isPaused,
    }));
  }, [conversation_id, data.items.length, enabled, updateState]);

  const lockInteraction = useCallback(() => {
    if (!enabled) {
      return;
    }

    interactionLockedRef.current = true;
    logCommandQueue(conversationKey, 'interaction-locked', {
      itemCount: stateRef.current.items.length,
    });
    setIsInteractionLocked(true);
  }, [conversation_id, enabled]);

  const unlockInteraction = useCallback(() => {
    if (!enabled) {
      return;
    }

    interactionLockedRef.current = false;
    logCommandQueue(conversationKey, 'interaction-unlocked', {
      itemCount: stateRef.current.items.length,
    });
    setIsInteractionLocked(false);
  }, [conversation_id, enabled]);

  const resetActiveExecution = useCallback(
    (reason: 'stop' | 'external-reset') => {
      executionGenerationRef.current += 1;
      const hadPendingTurn = executionGateRef.current.phase !== 'idle';

      // An optimistic stop may lower the visual busy flag before the backend has
      // released its turn handle. Keep the queue closed until turn.completed;
      // otherwise the next queued POST can still race the active turn and 409.
      if (reason === 'stop') {
        executionGateRef.current = reduceCommandQueueExecutionGate(executionGateRef.current, { type: 'stop' });
        logCommandQueue(conversationKey, 'execution-stop-pending', {
          pendingItemCount: stateRef.current.items.length,
        });
        setExecutionGateVersion((version) => version + 1);
        return;
      }

      // Reset/clear-context has changed the backend lifecycle boundary, but a
      // local success response alone does not prove that an old turn handle is
      // gone. Keep queued work behind an authoritative GET even when no local
      // gate was visible before the reset.
      executionGateRef.current = { phase: 'waiting_completion' };

      logCommandQueue(conversationKey, 'execution-reset', {
        reason,
        hadPendingTurn,
        pendingItemCount: stateRef.current.items.length,
      });
      setExecutionGateVersion((version) => version + 1);
    },
    [conversation_id]
  );

  useEffect(() => {
    if (
      !shouldDispatchConversationCommandQueue({
        enabled,
        isHydrated,
        isPaused: pausedRef.current,
        isBusy,
        gate: executionGateRef.current,
        isInteractionLocked: interactionLockedRef.current,
        itemCount: data.items.length,
      })
    ) {
      return;
    }

    const [nextCommand] = data.items;
    const executionGeneration = executionGenerationRef.current + 1;
    executionGenerationRef.current = executionGeneration;
    const isExecutionCurrent = (): boolean =>
      isCommandQueueExecutionCurrent({
        mounted: mountedRef.current,
        currentConversationId: executionConversationKeyRef.current,
        expectedConversationId: conversationKey,
        currentGeneration: executionGenerationRef.current,
        expectedGeneration: executionGeneration,
      });
    executionGateRef.current = reduceCommandQueueExecutionGate(executionGateRef.current, { type: 'begin' });

    // Mark the item before the POST. This phase is persisted with the same
    // idempotency key so a remount or an ambiguous response can replay the
    // exact immutable payload instead of minting a second message.
    const currentQueueState = normalizeQueueState(stateRef.current);
    const sourceItems = currentQueueState.items.some((item) => item.id === nextCommand.id)
      ? currentQueueState.items
      : [...currentQueueState.items, nextCommand];
    const dispatchingState = normalizeQueueState({
      ...currentQueueState,
      items: sourceItems.map((item) =>
        item.id === nextCommand.id ? updateQueueItemDelivery(item, 'dispatching') : item
      ),
    });
    if (!persistQueueState(conversationKey, dispatchingState)) {
      const pausedState = normalizeQueueState({
        ...dispatchingState,
        items: dispatchingState.items.map((item) =>
          item.id === nextCommand.id ? updateQueueItemDelivery(item, 'paused') : item
        ),
        isPaused: true,
      });
      executionGateRef.current = IDLE_EXECUTION_GATE;
      pausedRef.current = true;
      stateRef.current = pausedState;
      void mutate(pausedState, { revalidate: false });
      logCommandQueue(conversationKey, 'dispatch-paused', {
        reason: 'persistence-failed',
        item: summarizeQueuedCommand(nextCommand),
      });
      Message.warning(
        t('conversation.commandQueue.persistenceFailed', {
          defaultValue: 'The message was kept in the composer because the queue could not be saved. Try again.',
        })
      );
      setExecutionGateVersion((version) => version + 1);
      return;
    }
    stateRef.current = dispatchingState;
    pausedRef.current = dispatchingState.isPaused;
    void publishState(dispatchingState);

    logCommandQueue(conversationKey, 'dispatching', {
      item: summarizeQueuedCommand(nextCommand),
      remainingItemCount: data.items.length - 1,
    });
    // Keep the item durably queued while the request is in flight. If this hook
    // unmounts after the backend accepts the POST but before the response is
    // observed, the next mount replays this exact UUIDv7 idempotency key rather
    // than losing the command or creating another turn.
    void Promise.resolve()
      .then(() => {
        if (!isExecutionCurrent()) return;
        return onExecute(nextCommand, { isCurrent: isExecutionCurrent });
      })
      .then(async (deliveryDisposition) => {
        if (!isExecutionCurrent()) return;
        // `onExecute` resolves only after the HTTP request is accepted. Remove
        // the persisted item at that point, and only from the generation that
        // still owns this conversation.
        await updateState((state) =>
          isExecutionCurrent()
            ? {
                items: removeQueuedCommand(state.items, nextCommand.id),
                isPaused: false,
              }
            : state
        );
        if (!isExecutionCurrent()) return;
        if (deliveryDisposition === 'replayed_completed') {
          executionGateRef.current = IDLE_EXECUTION_GATE;
          logCommandQueue(conversationKey, 'completed-replay-acknowledged', {
            pendingItemCount: stateRef.current.items.length,
          });
          setExecutionGateVersion((version) => version + 1);
          return;
        }
        // turn.started normally moves the gate first. If that WS event was
        // missed, reconcile the accepted send against authoritative runtime so
        // the queue cannot remain in waiting_start forever. The same helper is
        // retried on the authoritative visual busy down-edge if this read fails.
        void reconcileActiveExecution();
      })
      .catch((error) => {
        if (!isExecutionCurrent() || executionGateRef.current.phase === 'idle') {
          return;
        }
        const failureKind = classifyConversationSendFailure(error);
        const currentState = normalizeQueueState(stateRef.current);
        const currentItem = currentState.items.find((item) => item.id === nextCommand.id) ?? nextCommand;
        const recovery = getQueueItemRecovery(currentItem);
        const nextRecovery = { ...recovery };
        let canRecover = false;
        if (failureKind === 'turn_admission_conflict') {
          canRecover = recovery.admission_attempts < MAX_ADMISSION_RECOVERY_ATTEMPTS;
          if (canRecover) nextRecovery.admission_attempts += 1;
        } else if (failureKind === 'ambiguous_transport') {
          canRecover = recovery.transport_attempts < MAX_TRANSPORT_RECOVERY_ATTEMPTS;
          if (canRecover) nextRecovery.transport_attempts += 1;
        }

        logCommandQueue(conversationKey, 'execute-failed', {
          item: summarizeQueuedCommand(nextCommand),
          failureKind,
          recovery,
          error: error instanceof Error ? error.message : String(error),
        });

        if (canRecover) {
          const recoveryState = normalizeQueueState({
            ...currentState,
            items: currentState.items.map((item) =>
              item.id === nextCommand.id
                ? updateQueueItemDelivery(
                    item,
                    failureKind === 'turn_admission_conflict' ? 'waiting_for_turn' : 'retrying',
                    nextRecovery
                  )
                : item
            ),
            isPaused: false,
          });
          if (!persistQueueState(conversationKey, recoveryState)) {
            const pausedState = normalizeQueueState({
              ...recoveryState,
              items: recoveryState.items.map((item) =>
                item.id === nextCommand.id ? updateQueueItemDelivery(item, 'paused', nextRecovery) : item
              ),
              isPaused: true,
            });
            executionGateRef.current = IDLE_EXECUTION_GATE;
            pausedRef.current = true;
            stateRef.current = pausedState;
            void publishState(pausedState);
            Message.warning(
              t('conversation.commandQueue.persistenceFailed', {
                defaultValue: 'The message was kept in the composer because the queue could not be saved. Try again.',
              })
            );
            setExecutionGateVersion((version) => version + 1);
            return;
          }

          executionGateRef.current = { phase: 'waiting_completion' };
          stateRef.current = recoveryState;
          pausedRef.current = false;
          void publishState(recoveryState);
          logCommandQueue(conversationKey, 'recovery-scheduled', {
            commandId: nextCommand.id,
            failureKind,
            admissionAttempts: nextRecovery.admission_attempts,
            transportAttempts: nextRecovery.transport_attempts,
          });
          setExecutionGateVersion((version) => version + 1);
          return;
        }

        const pausedItem = updateQueueItemDelivery(currentItem, 'paused', nextRecovery);
        const pausedState = normalizeQueueState({
          ...currentState,
          // The same item may already be present because it stays persisted
          // during dispatch. De-duplicate by id when restoring it.
          items: restoreQueuedCommand(currentState.items, pausedItem),
          isPaused: true,
        });
        const persisted = persistQueueState(conversationKey, pausedState);
        executionGateRef.current = IDLE_EXECUTION_GATE;
        pausedRef.current = true;
        stateRef.current = pausedState;
        void publishState(pausedState);
        if (!persisted) {
          Message.warning(
            t('conversation.commandQueue.persistenceFailed', {
              defaultValue: 'The message was kept in the composer because the queue could not be saved. Try again.',
            })
          );
        } else {
          const warningKey =
            failureKind === 'turn_admission_conflict'
              ? 'conversation.commandQueue.admissionRecoveryFailed'
              : failureKind === 'ambiguous_transport'
                ? 'conversation.commandQueue.transportRecoveryFailed'
                : 'conversation.commandQueue.pausedAfterFailure';
          const defaultValue =
            failureKind === 'turn_admission_conflict'
              ? 'The conversation is still busy. The message was kept in the queue; try again after the current turn finishes.'
              : failureKind === 'ambiguous_transport'
                ? 'The send result was unclear. The message was kept in the queue; retry it when the connection is stable.'
                : 'The next queued command could not start. Edit, reorder, or remove it to continue.';
          Message.warning(t(warningKey, { defaultValue }));
        }
        setExecutionGateVersion((version) => version + 1);
      });
  }, [
    conversation_id,
    data.items,
    enabled,
    executionGateVersion,
    isBusy,
    isHydrated,
    isInteractionLocked,
    mutate,
    onExecute,
    publishState,
    reconcileActiveExecution,
    t,
    updateState,
  ]);

  return {
    items: enabled ? data.items : [],
    isPaused: enabled ? data.isPaused : false,
    isInteractionLocked,
    hasPendingCommands: enabled ? data.items.length > 0 : false,
    enqueue,
    update,
    remove,
    clear,
    reorder,
    sendNow,
    pause,
    resume,
    lockInteraction,
    unlockInteraction,
    resetActiveExecution,
    reconcileActiveExecution,
  };
};
