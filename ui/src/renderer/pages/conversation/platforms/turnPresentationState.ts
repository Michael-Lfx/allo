/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { MessageId } from '@/common/types/ids';

/**
 * Shared presentation phases for a conversation turn across Nomi / ACP / Remote.
 * Maps wire events into one compact status rail the UI can render.
 */
export type TurnPresentationPhase =
  | 'idle'
  | 'local_pending'
  | 'accepted'
  | 'preparing'
  | 'thinking'
  | 'streaming'
  | 'tooling'
  | 'waiting_permission'
  | 'finalizing'
  | 'completed'
  | 'failed'
  | 'cancelled';

export type TurnPresentationState = {
  phase: TurnPresentationPhase;
  /** Authoritative backend turn root id once known. */
  activeTurnId?: MessageId;
  /** User request message that started this turn. */
  activeRequestMessageId?: MessageId;
  /** Temporary client id used before HTTP accept. */
  localRequestId?: string;
  detail?: string;
  /** Stream text finished but turn handle not yet released. */
  streamFinished: boolean;
  /** Composer should still show a stop control (strong busy). */
  showStop: boolean;
  /** Composer may accept enqueue / steer / new send. */
  composerInteractive: boolean;
  /** Compact status rail should be visible. */
  showStatusRail: boolean;
  startedAt?: number;
  firstTokenAt?: number;
  finishedAt?: number;
};

export type TurnPresentationEvent =
  | { type: 'localSubmit'; localRequestId: string; requestMessageId?: MessageId; at?: number }
  | { type: 'accepted'; requestMessageId: MessageId; turnId?: MessageId; at?: number }
  | { type: 'turnStarted'; turnId?: MessageId; phase?: string; state?: string; detail?: string; at?: number }
  | { type: 'preparing'; detail?: string }
  | { type: 'thinking'; detail?: string }
  | { type: 'streaming'; at?: number }
  | { type: 'tooling'; detail?: string }
  | { type: 'waitingPermission'; detail?: string }
  | { type: 'streamFinished'; at?: number }
  | { type: 'turnCompleted'; at?: number }
  | { type: 'failed'; detail?: string; at?: number }
  | { type: 'cancelled'; at?: number }
  | { type: 'reset' };

export const initialTurnPresentationState: TurnPresentationState = {
  phase: 'idle',
  streamFinished: false,
  showStop: false,
  composerInteractive: true,
  showStatusRail: false,
};

const ACTIVE_PHASES = new Set<TurnPresentationPhase>([
  'local_pending',
  'accepted',
  'preparing',
  'thinking',
  'streaming',
  'tooling',
  'waiting_permission',
  'finalizing',
]);

export function isTurnPresentationActive(state: TurnPresentationState): boolean {
  return ACTIVE_PHASES.has(state.phase);
}

function withBusyFlags(
  state: TurnPresentationState,
  phase: TurnPresentationPhase,
  patch: Partial<TurnPresentationState> = {}
): TurnPresentationState {
  const streamFinished = patch.streamFinished ?? state.streamFinished;
  const next: TurnPresentationState = {
    ...state,
    ...patch,
    phase,
    streamFinished,
  };

  switch (phase) {
    case 'idle':
    case 'completed':
    case 'failed':
    case 'cancelled':
      return {
        ...next,
        showStop: false,
        composerInteractive: true,
        showStatusRail: false,
        streamFinished: false,
      };
    case 'finalizing':
      return {
        ...next,
        showStop: false,
        composerInteractive: true,
        showStatusRail: true,
      };
    case 'local_pending':
    case 'accepted':
    case 'preparing':
    case 'thinking':
    case 'streaming':
    case 'tooling':
    case 'waiting_permission':
      return {
        ...next,
        showStop: !streamFinished,
        composerInteractive: streamFinished || phase === 'waiting_permission',
        showStatusRail: true,
      };
    default: {
      const _exhaustive: never = phase;
      return _exhaustive;
    }
  }
}

function mapWirePhase(phase?: string, state?: string): TurnPresentationPhase | null {
  const key = (phase || state || '').toLowerCase();
  switch (key) {
    case 'starting':
    case 'initializing':
    case 'preparing':
      return 'preparing';
    case 'thinking':
      return 'thinking';
    case 'streaming':
    case 'running':
    case 'ai_generating':
      return 'streaming';
    case 'tooling':
      return 'tooling';
    case 'waiting_permission':
    case 'waiting_confirmation':
    case 'ai_waiting_confirmation':
      return 'waiting_permission';
    default:
      return null;
  }
}

export function turnPresentationReducer(
  state: TurnPresentationState,
  event: TurnPresentationEvent
): TurnPresentationState {
  switch (event.type) {
    case 'reset':
      return { ...initialTurnPresentationState };

    case 'localSubmit':
      return withBusyFlags(state, 'local_pending', {
        localRequestId: event.localRequestId,
        activeRequestMessageId: event.requestMessageId ?? state.activeRequestMessageId,
        activeTurnId: undefined,
        detail: undefined,
        streamFinished: false,
        startedAt: event.at ?? Date.now(),
        firstTokenAt: undefined,
        finishedAt: undefined,
      });

    case 'accepted':
      return withBusyFlags(state, state.phase === 'local_pending' ? 'accepted' : state.phase, {
        activeRequestMessageId: event.requestMessageId,
        activeTurnId: event.turnId ?? state.activeTurnId,
        localRequestId: undefined,
      });

    case 'turnStarted': {
      const mapped = mapWirePhase(event.phase, event.state) ?? 'preparing';
      return withBusyFlags(state, mapped, {
        activeTurnId: event.turnId ?? state.activeTurnId,
        detail: event.detail || state.detail,
        startedAt: event.at ?? state.startedAt ?? Date.now(),
      });
    }

    case 'preparing':
      return withBusyFlags(state, 'preparing', { detail: event.detail ?? state.detail });

    case 'thinking':
      return withBusyFlags(state, 'thinking', { detail: event.detail ?? state.detail });

    case 'streaming':
      return withBusyFlags(state, 'streaming', {
        firstTokenAt: state.firstTokenAt ?? event.at ?? Date.now(),
        detail: undefined,
      });

    case 'tooling':
      return withBusyFlags(state, 'tooling', { detail: event.detail ?? state.detail });

    case 'waitingPermission':
      return withBusyFlags(state, 'waiting_permission', {
        detail: event.detail ?? state.detail,
      });

    case 'streamFinished':
      return withBusyFlags(state, 'finalizing', {
        streamFinished: true,
        detail: undefined,
      });

    case 'turnCompleted':
      return withBusyFlags(state, 'completed', {
        finishedAt: event.at ?? Date.now(),
        activeTurnId: undefined,
        localRequestId: undefined,
      });

    case 'failed':
      return withBusyFlags(state, 'failed', {
        detail: event.detail,
        finishedAt: event.at ?? Date.now(),
        activeTurnId: undefined,
        localRequestId: undefined,
      });

    case 'cancelled':
      return withBusyFlags(state, 'cancelled', {
        finishedAt: event.at ?? Date.now(),
        activeTurnId: undefined,
        localRequestId: undefined,
      });

    default: {
      const _exhaustive: never = event;
      return _exhaustive;
    }
  }
}

export function getTurnStatusLabel(
  phase: TurnPresentationPhase,
  detail?: string,
  t?: (key: string, options?: Record<string, unknown>) => string
): string {
  const translate =
    t ??
    ((key: string, options?: Record<string, unknown>) =>
      typeof options?.defaultValue === 'string' ? options.defaultValue : key);

  if (detail?.trim()) return detail.trim();

  switch (phase) {
    case 'local_pending':
      return translate('conversation.turnStatus.sending', { defaultValue: 'Sending…' });
    case 'accepted':
    case 'preparing':
      return translate('conversation.turnStatus.preparing', { defaultValue: 'Preparing…' });
    case 'thinking':
      return translate('conversation.turnStatus.thinking', { defaultValue: 'Thinking…' });
    case 'streaming':
      return translate('conversation.turnStatus.streaming', { defaultValue: 'Writing…' });
    case 'tooling':
      return translate('conversation.turnStatus.tooling', { defaultValue: 'Working…' });
    case 'waiting_permission':
      return translate('conversation.turnStatus.waitingPermission', {
        defaultValue: 'Waiting for your confirmation',
      });
    case 'finalizing':
      return translate('conversation.turnStatus.finalizing', {
        defaultValue: 'Saving result…',
      });
    case 'completed':
      return translate('conversation.turnStatus.completed', { defaultValue: 'Done' });
    case 'failed':
      return translate('conversation.turnStatus.failed', { defaultValue: 'Failed' });
    case 'cancelled':
      return translate('conversation.turnStatus.cancelled', { defaultValue: 'Stopped' });
    case 'idle':
      return '';
    default: {
      const _exhaustive: never = phase;
      return _exhaustive;
    }
  }
}
