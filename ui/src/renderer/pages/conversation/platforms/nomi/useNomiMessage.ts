/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { ConversationId, MessageId } from '@/common/types/ids';
import { ipcBridge } from '@/common';
import { transformMessage, transformUserCreatedEvent } from '@/common/chat/chatLib';
import { isToolGroupStatusActive, normalizeToolGroupStatus } from '@/common/chat/toolGroupStatus';
import { extractResponseTextChunk, optionalDisplayText, toDisplayText } from '@/common/chat/displayText';
import type { IResponseMessage } from '@/common/adapter/ipcBridge';
import type { TChatConversation, TokenUsageData } from '@/common/config/storage';
import { prefixedId, uuid } from '@/common/utils';
import { useAddOrUpdateMessage } from '@/renderer/pages/conversation/Messages/hooks';
import { getConversationOrNull, refreshConversationCache } from '@/renderer/pages/conversation/utils/conversationCache';
import {
  isCompleteMessageProjection,
  isConversationProcessing,
} from '@/renderer/pages/conversation/utils/conversationRuntime';
import { emitter } from '@/renderer/utils/emitter';
import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from 'react';
import type { ThoughtData } from '../thoughtTypes';
import { reconcileConversationTurnAfterStreamTerminal } from '../reconcileConversationTurnAfterStreamTerminal';
import {
  classifyAuthoritativeTurnCompletion,
  classifyAuthoritativeTurnStart,
  resolveVerifiedAuthoritativeTurnStart,
} from '../authoritativeTurnLifecyclePolicy';
import {
  beginTurnTiming,
  markTurnAbandonedBeforeFirstToken,
  markTurnAccepted as markFunnelTurnAccepted,
  markTurnFirstStatus,
  markTurnFirstToken,
  markTurnIdle,
  markTurnStreamFinished,
} from '@/renderer/utils/analytics/productFunnel';
import { processLocalCronResponse } from './localCronCommands';
import { initialNomiTurnState, isTurnRunning, nomiTurnReducer, type NomiTurnEvent } from './nomiTurnState';
import {
  initialTurnPresentationState,
  turnPresentationReducer,
  type TurnPresentationEvent,
} from '../turnPresentationState';

type NomiToolGroupRuntimeTool = {
  status: ReturnType<typeof normalizeToolGroupStatus>;
  name?: string;
  description?: string;
};

export const getNomiToolGroupRuntimeState = (data: unknown): {
  tools: NomiToolGroupRuntimeTool[];
  hasActive: boolean;
  hasAny: boolean;
  confirmingDescription?: string;
  executingDescription?: string;
} => {
  const tools = Array.isArray(data)
    ? data
        .filter((item): item is Record<string, unknown> => !!item && typeof item === 'object' && !Array.isArray(item))
        .map((tool) => ({
          status: normalizeToolGroupStatus(tool.status),
          ...(tool.name != null ? { name: toDisplayText(tool.name) } : {}),
          ...(tool.description != null ? { description: toDisplayText(tool.description) } : {}),
        }))
    : [];
  const hasActive = tools.some((tool) => isToolGroupStatusActive(tool.status));
  const confirmingTool = tools.find((tool) => tool.status === 'Confirming');
  const executingTool = tools.find((tool) => tool.status === 'Executing');

  return {
    tools,
    hasActive,
    hasAny: tools.length > 0,
    confirmingDescription: confirmingTool
      ? optionalDisplayText(confirmingTool.description) || optionalDisplayText(confirmingTool.name) || 'Tool execution'
      : undefined,
    executingDescription: executingTool
      ? optionalDisplayText(executingTool.description) || optionalDisplayText(executingTool.name) || 'Tool'
      : undefined,
  };
};

const normalizeThoughtData = (data: unknown): ThoughtData => {
  if (!data || typeof data !== 'object' || Array.isArray(data)) {
    return { subject: '', description: toDisplayText(data) };
  }
  const record = data as Record<string, unknown>;
  return {
    subject: record.subject != null ? toDisplayText(record.subject) : '',
    description: record.description != null ? toDisplayText(record.description) : '',
  };
};

export const useNomiMessage = (
  conversation_id: ConversationId,
  options?: {
    onError?: (message: IResponseMessage) => void;
    onConfigChanged?: (capabilities: Record<string, unknown>) => void;
    readOnly?: boolean;
  }
) => {
  const onError = options?.onError;
  const onConfigChanged = options?.onConfigChanged;
  const readOnly = options?.readOnly === true;
  const onConfigChangedRef = useRef(onConfigChanged);
  const addOrUpdateMessage = useAddOrUpdateMessage();
  // Single source of truth for the turn's activity state (design §3.2): a pure
  // reducer over lifecycle events replaces three hand-synced booleans.
  const [turnState, dispatchTurn] = useReducer(nomiTurnReducer, initialNomiTurnState);
  const [presentation, dispatchPresentation] = useReducer(
    turnPresentationReducer,
    initialTurnPresentationState
  );
  const [hasHydratedRunningState, setHasHydratedRunningState] = useState(false);
  const [thought, setThought] = useState<ThoughtData>({
    description: '',
    subject: '',
  });
  const [tokenUsage, setTokenUsage] = useState<TokenUsageData | null>(null);
  const [activeTurnId, setActiveTurnId] = useState<MessageId | undefined>();
  const [activeRequestMessageId, setActiveRequestMessageId] = useState<MessageId | undefined>();
  // Current active message ID to filter out events from old requests (prevents aborted request events from interfering with new ones)
  const activeMsgIdRef = useRef<string | null>(null);
  const rootTurnIdRef = useRef<MessageId | null>(null);
  const timingRequestKeyRef = useRef<string | null>(null);
  const presentationRef = useRef(presentation);
  const awaitingBackendTurnRef = useRef(false);
  const turnClosedRef = useRef(false);
  const cancelledTurnIdsRef = useRef(new Set<MessageId>());
  const rejectUnannouncedStartRef = useRef(false);
  const verifyUnannouncedStartRuntimeRef = useRef(false);
  const turnLifecycleGenerationRef = useRef(0);
  const turnStartGenerationRef = useRef(0);
  const turnCompletionGenerationRef = useRef(0);
  const turnReconcileSequenceRef = useRef(0);
  const mountedRef = useRef(true);
  const messageBufferRef = useRef(new Map<string, string>());
  const processedCronMsgIdsRef = useRef(new Set<string>());

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      turnLifecycleGenerationRef.current += 1;
      turnReconcileSequenceRef.current += 1;
    };
  }, []);

  // Mirror the reducer state into a ref so the (non-resubscribing) stream
  // closure can read the current turn state without being a dependency.
  const turnStateRef = useRef(turnState);
  useEffect(() => {
    turnStateRef.current = turnState;
  }, [turnState]);

  useEffect(() => {
    presentationRef.current = presentation;
  }, [presentation]);

  useEffect(() => {
    onConfigChangedRef.current = onConfigChanged;
  }, [onConfigChanged]);

  const dispatchPresentationEvent = useCallback((event: TurnPresentationEvent) => {
    dispatchPresentation(event);
  }, []);

  const bindTimingKey = useCallback((requestKey: string | null) => {
    timingRequestKeyRef.current = requestKey;
  }, []);

  const resolveTimingKey = useCallback(() => {
    return (
      timingRequestKeyRef.current ??
      presentationRef.current.activeRequestMessageId ??
      presentationRef.current.localRequestId ??
      activeMsgIdRef.current
    );
  }, []);

  // Throttle thought updates to reduce render frequency
  const thoughtThrottleRef = useRef<{
    lastUpdate: number;
    pending: ThoughtData | null;
    timer: ReturnType<typeof setTimeout> | null;
  }>({ lastUpdate: 0, pending: null, timer: null });

  const throttledSetThought = useMemo(() => {
    const THROTTLE_MS = 50; // 50ms throttle interval
    return (data: ThoughtData) => {
      const now = Date.now();
      const ref = thoughtThrottleRef.current;

      if (now - ref.lastUpdate >= THROTTLE_MS) {
        ref.lastUpdate = now;
        ref.pending = null;
        if (ref.timer) {
          clearTimeout(ref.timer);
          ref.timer = null;
        }
        setThought(data);
      } else {
        ref.pending = data;
        if (!ref.timer) {
          ref.timer = setTimeout(
            () => {
              ref.lastUpdate = Date.now();
              ref.timer = null;
              if (ref.pending) {
                setThought(ref.pending);
                ref.pending = null;
              }
            },
            THROTTLE_MS - (now - ref.lastUpdate)
          );
        }
      }
    };
  }, []);

  // Cleanup throttle timer
  useEffect(() => {
    return () => {
      if (thoughtThrottleRef.current.timer) {
        clearTimeout(thoughtThrottleRef.current.timer);
      }
    };
  }, []);

  // Combined running state: waiting for response OR stream is running OR tools are active
  const running = isTurnRunning(turnState);

  // Set current active message ID
  const setActiveMsgId = useCallback((msgId: string | null) => {
    activeMsgIdRef.current = msgId;
    if (msgId) {
      try {
        setActiveRequestMessageId(msgId as MessageId);
      } catch {
        setActiveRequestMessageId(undefined);
      }
    } else {
      setActiveRequestMessageId(undefined);
    }
  }, []);

  const notifyLocalSubmit = useCallback(
    (localRequestId: string, requestMessageId?: MessageId) => {
      bindTimingKey(localRequestId);
      beginTurnTiming(localRequestId, {
        conversation_type: 'nomi',
        cold_start: !hasHydratedRunningState,
      });
      if (requestMessageId) {
        setActiveRequestMessageId(requestMessageId);
        activeMsgIdRef.current = requestMessageId;
      }
      dispatchPresentation({
        type: 'localSubmit',
        localRequestId,
        requestMessageId,
      });
    },
    [bindTimingKey, hasHydratedRunningState]
  );

  const notifyAccepted = useCallback(
    (requestMessageId: MessageId, turnId?: MessageId) => {
      const timingKey = resolveTimingKey() ?? requestMessageId;
      bindTimingKey(timingKey);
      markFunnelTurnAccepted(timingKey, { conversation_type: 'nomi' });
      setActiveRequestMessageId(requestMessageId);
      activeMsgIdRef.current = requestMessageId;
      if (turnId) {
        setActiveTurnId(turnId);
        rootTurnIdRef.current = turnId;
      }
      dispatchPresentation({
        type: 'accepted',
        requestMessageId,
        turnId,
      });
    },
    [bindTimingKey, resolveTimingKey]
  );

  const notifyFailed = useCallback(
    (detail?: string) => {
      const timingKey = resolveTimingKey();
      if (timingKey) {
        markTurnAbandonedBeforeFirstToken(timingKey);
        markTurnIdle(timingKey, 'failed');
        timingRequestKeyRef.current = null;
      }
      setActiveTurnId(undefined);
      setActiveRequestMessageId(undefined);
      dispatchPresentation({ type: 'failed', detail });
    },
    [resolveTimingKey]
  );

  const dispatchTurnIfOpen = useCallback((event: NomiTurnEvent) => {
    if (turnClosedRef.current && !awaitingBackendTurnRef.current) return;
    dispatchTurn(event);
  }, []);

  const settleCompletedTurn = useCallback(() => {
    turnLifecycleGenerationRef.current += 1;
    turnCompletionGenerationRef.current += 1;
    turnReconcileSequenceRef.current += 1;
    rootTurnIdRef.current = null;
    awaitingBackendTurnRef.current = false;
    turnClosedRef.current = true;
    rejectUnannouncedStartRef.current = false;
    activeMsgIdRef.current = null;
    setActiveTurnId(undefined);
    setActiveRequestMessageId(undefined);
    dispatchTurn({ type: 'finish' });
    const timingKey = resolveTimingKey();
    if (timingKey) {
      markTurnIdle(timingKey, 'completed');
      timingRequestKeyRef.current = null;
    }
    dispatchPresentation({ type: 'turnCompleted' });
    setThought({ subject: '', description: '' });
  }, [resolveTimingKey]);

  const reconcileAfterStreamTerminal = useCallback(() => {
    const generation = turnLifecycleGenerationRef.current;
    const sequence = turnReconcileSequenceRef.current + 1;
    turnReconcileSequenceRef.current = sequence;
    void reconcileConversationTurnAfterStreamTerminal(
      conversation_id,
      () =>
        mountedRef.current &&
        turnLifecycleGenerationRef.current === generation &&
        turnReconcileSequenceRef.current === sequence,
      settleCompletedTurn
    );
  }, [conversation_id, settleCompletedTurn]);

  const markTurnAccepted = useCallback(
    () => {
      if (!awaitingBackendTurnRef.current || rejectUnannouncedStartRef.current) return;
      if (!verifyUnannouncedStartRuntimeRef.current) turnLifecycleGenerationRef.current += 1;
      rootTurnIdRef.current = null;
      awaitingBackendTurnRef.current = false;
      const generation = turnLifecycleGenerationRef.current;
      const sequence = turnReconcileSequenceRef.current + 1;
      turnReconcileSequenceRef.current = sequence;
      void reconcileConversationTurnAfterStreamTerminal(
        conversation_id,
        () =>
          mountedRef.current &&
          turnLifecycleGenerationRef.current === generation &&
          turnReconcileSequenceRef.current === sequence,
        settleCompletedTurn
      );
    },
    [conversation_id, settleCompletedTurn]
  );

  const processCompletedAssistantMessage = useCallback(
    async (msgId: MessageId) => {
      if (readOnly || !msgId || processedCronMsgIdsRef.current.has(msgId)) {
        return;
      }

      const rawContent = messageBufferRef.current.get(msgId) ?? '';
      if (!rawContent.trim()) {
        return;
      }

      processedCronMsgIdsRef.current.add(msgId);

      try {
        const result = await processLocalCronResponse(conversation_id, rawContent);
        if (result.displayContent !== undefined && result.displayContent !== rawContent) {
          addOrUpdateMessage({
            id: uuid(),
            msg_id: msgId,
            type: 'text',
            position: 'left',
            conversation_id,
            created_at: Date.now(),
            content: {
              content: result.displayContent,
              replace: true,
            },
          });
        }

        for (const response of result.systemResponses) {
          addOrUpdateMessage(
            {
              id: prefixedId('msg'),
              type: 'tips',
              position: 'center',
              conversation_id,
              created_at: Date.now(),
              content: {
                content: response,
                type: response.startsWith('❌') ? 'error' : 'success',
              },
            },
            true
          );
        }
      } catch {
        processedCronMsgIdsRef.current.delete(msgId);
      }
    },
    [addOrUpdateMessage, conversation_id, readOnly]
  );

  useEffect(() => {
    return ipcBridge.conversation.userCreated.on((event) => {
      addOrUpdateMessage(transformUserCreatedEvent(event, conversation_id));
    });
  }, [conversation_id, addOrUpdateMessage]);

  useEffect(() => {
    return ipcBridge.conversation.responseStream.on((message) => {
      if (conversation_id !== message.conversation_id) {
        return;
      }

      // Filter out events not belonging to current active request (prevents aborted events from interfering)
      // Note: only filter out thought and start messages, other messages must be rendered
      if (activeMsgIdRef.current && message.msg_id && message.msg_id !== activeMsgIdRef.current) {
        if (message.type === 'thought') {
          return;
        }
      }

      if ((message.type === 'content' || message.type === 'text') && message.msg_id) {
        const chunk = extractResponseTextChunk(message.data);

        if (chunk) {
          const previous = messageBufferRef.current.get(message.msg_id) ?? '';
          messageBufferRef.current.set(message.msg_id, previous + chunk);
        }
      }

      switch (message.type) {
        case 'thought':
          dispatchTurnIfOpen({ type: 'activity' });
          {
            const timingKey = resolveTimingKey();
            if (timingKey) markTurnFirstStatus(timingKey, 'thinking');
            dispatchPresentationEvent({
              type: 'thinking',
              detail: normalizeThoughtData(message.data).description || undefined,
            });
          }
          throttledSetThought(normalizeThoughtData(message.data));
          break;
        case 'start':
          dispatchTurnIfOpen({ type: 'activity' });
          {
            const timingKey = resolveTimingKey();
            if (timingKey) markTurnFirstStatus(timingKey, 'preparing');
            dispatchPresentationEvent({ type: 'preparing' });
          }
          // Don't reset waitingResponse here - let tool completion flow handle it
          break;
        case 'turn_completed':
          {
            // Phase 3 observability: the engine emits one turn_completed per turn
            // carrying real aggregate metrics. This is the genuine source of token
            // usage for nomi turns (the finish event has never carried usage) —
            // it updates the send-box metrics chip and persists for rehydration.
            const metrics = message.data as
              | {
                  elapsed_ms?: number;
                  input_tokens?: number;
                  output_tokens?: number;
                  cache_creation_tokens?: number;
                  cache_read_tokens?: number;
                  context_tokens?: number;
                  context_window?: number;
                }
              | undefined;
            if (metrics && typeof metrics === 'object') {
              const inputTokens = metrics.input_tokens || 0;
              const outputTokens = metrics.output_tokens || 0;
              const newTokenUsage: TokenUsageData = {
                total_tokens: inputTokens + outputTokens,
                input_tokens: metrics.input_tokens,
                output_tokens: metrics.output_tokens,
                cache_creation_tokens: metrics.cache_creation_tokens,
                cache_read_tokens: metrics.cache_read_tokens,
                elapsed_ms: metrics.elapsed_ms,
                context_tokens: metrics.context_tokens,
                context_window: metrics.context_window,
              };
              setTokenUsage(newTokenUsage);
              if (!readOnly) {
                emitter.emit('nomi.usage.updated', { conversation_id, tokenUsage: newTokenUsage });
                void ipcBridge.conversation.update
                  .invoke({
                    id: conversation_id,
                    updates: {
                      extra: { last_token_usage: newTokenUsage } as TChatConversation['extra'],
                    },
                  })
                  .then((ok) => {
                    if (ok) {
                      void refreshConversationCache(conversation_id);
                    }
                  })
                  .catch((error) => {
                    console.warn('[nomi] failed to persist last_token_usage', error);
                  });
              }
            }
          }
          break;
        case 'finish':
          {
            // Stream completion can precede backend turn-handle release.
            setThought({ subject: '', description: '' });
            const timingKey = resolveTimingKey();
            if (timingKey) markTurnStreamFinished(timingKey);
            dispatchPresentationEvent({ type: 'streamFinished' });
            if (message.msg_id) {
              void processCompletedAssistantMessage(message.msg_id);
            }
            reconcileAfterStreamTerminal();
          }
          break;
        case 'tool_group':
          {
            // Check if any tools are executing or awaiting confirmation
            const toolState = getNomiToolGroupRuntimeState(message.data);
            dispatchTurnIfOpen({ type: 'toolGroup', hasActive: toolState.hasActive, hasAny: toolState.hasAny });

            // If tools are awaiting confirmation, update thought hint
            if (toolState.confirmingDescription) {
              const timingKey = resolveTimingKey();
              if (timingKey) markTurnFirstStatus(timingKey, 'waiting_permission');
              dispatchPresentationEvent({
                type: 'waitingPermission',
                detail: toolState.confirmingDescription,
              });
              setThought({
                subject: 'Awaiting Confirmation',
                // Prefer the contextual description (file/command/pattern) over the
                // bare tool name so the status reads e.g. "edit src/auth.ts".
                description: toolState.confirmingDescription,
              });
            } else if (toolState.hasActive) {
              const timingKey = resolveTimingKey();
              if (timingKey) markTurnFirstStatus(timingKey, 'tooling');
              dispatchPresentationEvent({
                type: 'tooling',
                detail: toolState.executingDescription,
              });
              if (toolState.executingDescription) {
                setThought({
                  subject: 'Executing',
                  description: toolState.executingDescription,
                });
              }
            } else if (!turnStateRef.current.streamRunning) {
              // All tools completed and stream stopped, clear thought
              setThought({ subject: '', description: '' });
            }

            // Continue passing message to message list update
            addOrUpdateMessage(transformMessage(message));
          }
          break;
        case 'permission':
        case 'acp_permission':
          dispatchTurnIfOpen({ type: 'activity' });
          {
            const timingKey = resolveTimingKey();
            if (timingKey) markTurnFirstStatus(timingKey, 'waiting_permission');
            dispatchPresentationEvent({ type: 'waitingPermission' });
          }
          // Backend nomi emits wire type 'acp_permission' but the payload is
          // Confirmation-shaped (legacy), which matches MessagePermission, not
          // MessageAcpPermission. Re-tag so transformMessage routes it correctly.
          addOrUpdateMessage(transformMessage({ ...message, type: 'permission' }));
          break;
        case 'config_changed':
          onConfigChangedRef.current?.(message.data as Record<string, unknown>);
          break;
        default: {
          if (message.type === 'error') {
            setThought({ subject: '', description: '' });
            const timingKey = resolveTimingKey();
            if (timingKey) {
              markTurnIdle(timingKey, 'failed');
              timingRequestKeyRef.current = null;
            }
            dispatchPresentationEvent({
              type: 'failed',
              detail: typeof message.data === 'string' ? message.data : undefined,
            });
            onError?.(message as IResponseMessage);
            reconcileAfterStreamTerminal();
          } else if (message.type === 'content') {
            // A terminal Agent Execution report is a self-contained projection,
            // not a new model stream. Render it without re-raising the send-box
            // busy state; ordinary stream content still marks the turn active.
            const streamComplete = isCompleteMessageProjection(message);
            dispatchTurnIfOpen({
              type: 'content',
              streamComplete,
            });
            if (!streamComplete) {
              const timingKey = resolveTimingKey();
              if (timingKey) markTurnFirstToken(timingKey);
              dispatchPresentationEvent({ type: 'streaming' });
            }
          } else {
            // Any other non-error output: keep the turn marked running (handles
            // events that arrive after a premature finish).
            dispatchTurnIfOpen({ type: 'activity' });
            if (message.type === 'text') {
              const timingKey = resolveTimingKey();
              if (timingKey) markTurnFirstToken(timingKey);
              dispatchPresentationEvent({ type: 'streaming' });
            }
          }
          // Backend handles persistence, Frontend only updates UI
          addOrUpdateMessage(transformMessage(message));
          break;
        }
      }
    });
    // Note: turn state is read via turnStateRef to avoid re-subscription
  }, [
    conversation_id,
    addOrUpdateMessage,
    dispatchPresentationEvent,
    dispatchTurnIfOpen,
    onError,
    processCompletedAssistantMessage,
    readOnly,
    reconcileAfterStreamTerminal,
    resolveTimingKey,
  ]);

  useEffect(() => {
    let disposed = false;
    const unsubscribe = ipcBridge.conversation.turnStarted.on((event) => {
      if (event.conversation_id !== conversation_id) return;
      const startAction = classifyAuthoritativeTurnStart({
        turnId: event.turn_id,
        cancelledTurnIds: cancelledTurnIdsRef.current,
        rejectUnannouncedStart: rejectUnannouncedStartRef.current,
        awaitingBackendTurn: awaitingBackendTurnRef.current,
        verifyUnannouncedStartRuntime: verifyUnannouncedStartRuntimeRef.current,
      });
      if (startAction === 'ignore') return;

      const acceptStart = () => {
        turnStartGenerationRef.current += 1;
        turnLifecycleGenerationRef.current += 1;
        rootTurnIdRef.current = event.turn_id ?? null;
        setActiveTurnId(event.turn_id);
        awaitingBackendTurnRef.current = false;
        turnClosedRef.current = false;
        rejectUnannouncedStartRef.current = false;
        verifyUnannouncedStartRuntimeRef.current = false;
        dispatchTurn({ type: 'activity' });
        const timingKey = timingRequestKeyRef.current ?? event.turn_id ?? activeMsgIdRef.current;
        if (timingKey) markTurnFirstStatus(timingKey, event.phase ?? event.state);
        dispatchPresentation({
          type: 'turnStarted',
          turnId: event.turn_id,
          phase: event.phase,
          state: event.state,
          detail: event.detail,
        });
      };

      if (startAction === 'accept') {
        acceptStart();
        return;
      }

      const generation = turnLifecycleGenerationRef.current;
      void getConversationOrNull(conversation_id)
        .then((conversation) => {
          if (
            disposed ||
            turnLifecycleGenerationRef.current !== generation ||
            !verifyUnannouncedStartRuntimeRef.current ||
            resolveVerifiedAuthoritativeTurnStart({
              runtimeIsProcessing: isConversationProcessing(conversation),
              eventProcessingStartedAt: event.runtime.processing_started_at,
              runtimeProcessingStartedAt: conversation?.runtime?.processing_started_at,
            }) !== 'accept'
          ) {
            return;
          }
          acceptStart();
        })
        .catch((error) => {
          if (disposed) return;
          console.warn('[useNomiMessage] Failed to verify unannounced turn start:', error);
        });
    });
    return () => {
      disposed = true;
      unsubscribe();
    };
  }, [conversation_id]);

  useEffect(() => {
    let disposed = false;

    const unsubscribe = ipcBridge.conversation.turnCompleted.on((event) => {
      if (event.conversation_id !== conversation_id || event.runtime.is_processing) return;

      const rootTurnId = rootTurnIdRef.current;
      const awaitingBackendTurn = awaitingBackendTurnRef.current;
      const action = classifyAuthoritativeTurnCompletion({
        rootTurnId,
        completedTurnId: event.turn_id,
        awaitingBackendTurn,
      });
      if (action === 'settle') {
        settleCompletedTurn();
        return;
      }
      if (action === 'ignore') return;

      const observedRootTurnId = rootTurnId;
      const observedAwaitingBackendTurn = awaitingBackendTurn;
      const generation = turnLifecycleGenerationRef.current;
      const sequence = turnReconcileSequenceRef.current + 1;
      turnReconcileSequenceRef.current = sequence;
      void reconcileConversationTurnAfterStreamTerminal(
        conversation_id,
        () =>
          !disposed &&
          mountedRef.current &&
          turnLifecycleGenerationRef.current === generation &&
          turnReconcileSequenceRef.current === sequence &&
          rootTurnIdRef.current === observedRootTurnId &&
          awaitingBackendTurnRef.current === observedAwaitingBackendTurn,
        settleCompletedTurn
      );
    });

    return () => {
      disposed = true;
      unsubscribe();
    };
  }, [conversation_id, settleCompletedTurn]);

  useEffect(() => {
    let cancelled = false;

    // Clear turn state on conversation switch so a previous conversation's
    // running state cannot bleed into this one; the raise-only `hydrate` below
    // then merges the backend status with any send that races the async query.
    dispatchTurn({ type: 'reset' });
    dispatchPresentation({ type: 'reset' });
    turnLifecycleGenerationRef.current += 1;
    const hydrationGeneration = turnLifecycleGenerationRef.current;
    setThought({ subject: '', description: '' });
    setTokenUsage(null);
    setHasHydratedRunningState(false);
    setActiveTurnId(undefined);
    setActiveRequestMessageId(undefined);
    timingRequestKeyRef.current = null;
    rootTurnIdRef.current = null;
    awaitingBackendTurnRef.current = false;
    turnClosedRef.current = false;
    cancelledTurnIdsRef.current.clear();
    rejectUnannouncedStartRef.current = false;
    verifyUnannouncedStartRuntimeRef.current = false;

    // Check actual conversation status from backend before resetting all running states
    // to avoid flicker when switching to a running conversation
    void getConversationOrNull(conversation_id).then((res) => {
      if (cancelled) {
        return;
      }
      if (turnLifecycleGenerationRef.current !== hydrationGeneration) {
        setHasHydratedRunningState(true);
        return;
      }

      if (!res) {
        // No conversation record — already reset at effect start; just mark hydrated.
        setHasHydratedRunningState(true);
        return;
      }
      const isRunning = isConversationProcessing(res);
      // A send issued between this conversation mounting and this async query
      // resolving has already raised the spinner (executeCommand →
      // setWaitingResponse(true)). The query was fired BEFORE that send, so its
      // is_processing=false is stale — must NOT clobber a locally-raised running
      // state, or a brand-new conversation's first message shows no "正在处理"
      // indicator until the first live stream event arrives. `hydrate` is
      // raise-only, so it ORs the backend status onto whatever is already set.
      dispatchTurn({ type: 'hydrate', isRunning });
      // Load persisted token usage stats
      if (res.type === 'nomi' && res.extra?.last_token_usage) {
        const { last_token_usage } = res.extra;
        if (last_token_usage.total_tokens > 0) {
          setTokenUsage(last_token_usage);
        }
      }
      setHasHydratedRunningState(true);
    });

    return () => {
      cancelled = true;
    };
  }, [conversation_id]);

  const resetState = useCallback(() => {
    turnLifecycleGenerationRef.current += 1;
    const rootTurnId = rootTurnIdRef.current;
    if (rootTurnId) {
      const cancelled = cancelledTurnIdsRef.current;
      cancelled.add(rootTurnId);
      if (cancelled.size > 32) {
        const oldest = cancelled.values().next().value;
        if (oldest) cancelled.delete(oldest);
      }
    }
    awaitingBackendTurnRef.current = false;
    turnClosedRef.current = true;
    rejectUnannouncedStartRef.current = true;
    verifyUnannouncedStartRuntimeRef.current = rootTurnId === null;
    dispatchTurn({ type: 'reset' });
    const timingKey = resolveTimingKey();
    if (timingKey) {
      markTurnAbandonedBeforeFirstToken(timingKey);
      markTurnIdle(timingKey, 'cancelled');
      timingRequestKeyRef.current = null;
    }
    dispatchPresentation({ type: 'cancelled' });
    setActiveTurnId(undefined);
    setActiveRequestMessageId(undefined);
    setThought({ subject: '', description: '' });
    // Clear active message ID to prevent filtering events from new messages after stop
    activeMsgIdRef.current = null;
  }, [resolveTimingKey]);

  // External setter used by the send box to raise the spinner on submit.
  const setWaitingResponse = useCallback((value: boolean) => {
    turnLifecycleGenerationRef.current += 1;
    if (value) {
      turnStartGenerationRef.current += 1;
      rootTurnIdRef.current = null;
      setActiveTurnId(undefined);
      awaitingBackendTurnRef.current = true;
      turnClosedRef.current = false;
      rejectUnannouncedStartRef.current = false;
    } else {
      rootTurnIdRef.current = null;
      setActiveTurnId(undefined);
      awaitingBackendTurnRef.current = false;
      turnClosedRef.current = true;
      rejectUnannouncedStartRef.current = false;
    }
    dispatchTurn({ type: 'setWaiting', value });
  }, []);

  const restoreRunningAfterStopFailure = useCallback(() => {
    turnLifecycleGenerationRef.current += 1;
    const rootTurnId = rootTurnIdRef.current;
    if (rootTurnId) cancelledTurnIdsRef.current.delete(rootTurnId);
    awaitingBackendTurnRef.current = false;
    turnClosedRef.current = false;
    rejectUnannouncedStartRef.current = false;
    verifyUnannouncedStartRuntimeRef.current = false;
    dispatchTurn({ type: 'hydrate', isRunning: true });
    if (rootTurnId) setActiveTurnId(rootTurnId);
    const generation = turnLifecycleGenerationRef.current;
    const sequence = turnReconcileSequenceRef.current + 1;
    turnReconcileSequenceRef.current = sequence;
    void reconcileConversationTurnAfterStreamTerminal(
      conversation_id,
      () =>
        mountedRef.current &&
        turnLifecycleGenerationRef.current === generation &&
        turnReconcileSequenceRef.current === sequence,
      settleCompletedTurn
    );
  }, [conversation_id, settleCompletedTurn]);

  const confirmStopped = useCallback(() => {
    turnLifecycleGenerationRef.current += 1;
    rootTurnIdRef.current = null;
    awaitingBackendTurnRef.current = false;
    turnClosedRef.current = true;
    rejectUnannouncedStartRef.current = false;
    setActiveTurnId(undefined);
    setActiveRequestMessageId(undefined);
    dispatchTurn({ type: 'reset' });
    dispatchPresentation({ type: 'cancelled' });
  }, []);

  const getTurnStartGeneration = useCallback(() => turnStartGenerationRef.current, []);
  const getTurnCompletionGeneration = useCallback(() => turnCompletionGenerationRef.current, []);

  return {
    thought,
    setThought,
    running,
    hasHydratedRunningState,
    tokenUsage,
    presentation,
    activeTurnId,
    activeRequestMessageId,
    setActiveMsgId,
    markTurnAccepted,
    notifyLocalSubmit,
    notifyAccepted,
    notifyFailed,
    setWaitingResponse,
    resetState,
    confirmStopped,
    restoreRunningAfterStopFailure,
    getTurnStartGeneration,
    getTurnCompletionGeneration,
  };
};

export type NomiMessageRuntime = ReturnType<typeof useNomiMessage>;
