import type { ConversationId } from '@/common/types/ids';

export type PendingConversationStage = 'validating' | 'creating' | 'configuring' | 'opening';

/**
 * Snapshot of the message the user just submitted from the Guid composer, used
 * to render an immediate conversation-shaped loading state while the backend
 * mints the conversation entity.
 */
export type PendingConversation = {
  /** The first message the user typed (echoed as a right-aligned bubble). */
  input: string;
  /** Attachment paths, if any — shown as a small count under the bubble. */
  files?: string[];
  /**
   * Whether this entry will send `input` as the conversation's first turn.
   * AutoWork entries start a backend loop WITHOUT sending a first message, so
   * the loading caption differs ("正在启动 AutoWork…" vs "正在创建会话…").
   */
  sendsInitialMessage: boolean;
  /** Real client-side milestone reached by the create flow. */
  stage?: PendingConversationStage;
};

export type PendingTransitionPhase = 'idle' | 'pending' | 'awaiting-reveal' | 'exiting';

export type PendingTransitionSnapshot = {
  phase: PendingTransitionPhase;
  /** Stays non-null through 'exiting' so the overlay fades out with content intact. */
  pending: PendingConversation | null;
};

export type PendingTransitionOutcome = 'revealed' | 'timeout' | 'aborted';

export type PendingTransitionControllerOptions = {
  onChange: (snapshot: PendingTransitionSnapshot) => void;
  /** Hard fallback when the destination never signals reveal. Default 1500. */
  revealTimeoutMs?: number;
  /** Exit fade duration; a resolver so the provider can honor prefers-reduced-motion (→ 0). */
  exitDurationMs?: () => number;
  /** Called once when a transition settles (reveal / timeout / abort). */
  onSettled?: (outcome: PendingTransitionOutcome) => void;
  /** Subscribes the reveal listener; must return an unsubscribe fn. */
  subscribeReveal: (fn: (payload: { conversation_id: ConversationId }) => void) => () => void;
  // Injectable timers keep the controller testable without fake globals.
  setTimeoutFn?: (fn: () => void, ms: number) => unknown;
  clearTimeoutFn?: (handle: unknown) => void;
};

export type PendingTransitionController = {
  getSnapshot: () => PendingTransitionSnapshot;
  /** Show the loading overlay immediately (called synchronously on send). */
  begin: (payload: PendingConversation) => void;
  /** Advance the overlay only when the corresponding operation actually begins. */
  advance: (stage: PendingConversationStage) => void;
  /** Record the freshly minted conversation id so reveal events can be id-matched. */
  attach: (conversationId: ConversationId) => void;
  /** Success path, called after navigate is dispatched. Arms the reveal handshake. */
  end: () => void;
  /** Failure path. Drops the overlay instantly (no fade) to reveal the Guid composer. */
  abort: () => void;
  /** Subscribe the reveal listener (effect mount); safe to call again after stop. */
  start: () => void;
  /** Unsubscribe + clear timers (effect cleanup / unmount safety). */
  stop: () => void;
};

const DEFAULT_REVEAL_TIMEOUT_MS = 1500;
const DEFAULT_EXIT_DURATION_MS = 140;

/**
 * State machine behind the Guid→conversation transition overlay:
 *
 *   idle ─begin→ pending ─end→ awaiting-reveal ─reveal/timeout→ exiting ─→ idle
 *                    │                                              ↑
 *                    └────────────── abort (sync, no fade) ─────────┘
 *
 * The old implementation tore the overlay down on a fixed 280 ms timer after
 * navigate(), racing the destination's first-bubble commit (which is POST-gated)
 * and flashing an empty message list. The handshake waits for the destination's
 * `conversation.transition.reveal` signal instead, with a timeout backstop.
 */
export const createPendingTransitionController = (
  options: PendingTransitionControllerOptions,
): PendingTransitionController => {
  const {
    onChange,
    onSettled,
    subscribeReveal,
    revealTimeoutMs = DEFAULT_REVEAL_TIMEOUT_MS,
    exitDurationMs = () => DEFAULT_EXIT_DURATION_MS,
    setTimeoutFn = (fn, ms) => setTimeout(fn, ms),
    clearTimeoutFn = (handle) => clearTimeout(handle as Parameters<typeof clearTimeout>[0]),
  } = options;

  let phase: PendingTransitionPhase = 'idle';
  let pending: PendingConversation | null = null;
  let attachedId: ConversationId | undefined;
  let revealReceived = false;
  let revealTimer: unknown;
  let exitTimer: unknown;
  let unsubscribeReveal: (() => void) | undefined;

  const emit = () => {
    onChange({ phase, pending });
  };

  const clearRevealTimer = () => {
    if (revealTimer !== undefined) {
      clearTimeoutFn(revealTimer);
      revealTimer = undefined;
    }
  };

  const clearExitTimer = () => {
    if (exitTimer !== undefined) {
      clearTimeoutFn(exitTimer);
      exitTimer = undefined;
    }
  };

  const startExit = (outcome: PendingTransitionOutcome) => {
    clearRevealTimer();
    phase = 'exiting';
    onSettled?.(outcome);
    emit();
    const duration = Math.max(0, exitDurationMs());
    exitTimer = setTimeoutFn(() => {
      exitTimer = undefined;
      phase = 'idle';
      pending = null;
      attachedId = undefined;
      emit();
    }, duration);
  };

  const matchesAttached = (conversationId: ConversationId): boolean =>
    attachedId === undefined || attachedId === conversationId;

  const handleReveal = (payload: { conversation_id: ConversationId }) => {
    if (phase === 'awaiting-reveal') {
      if (!matchesAttached(payload.conversation_id)) return;
      startExit('revealed');
      return;
    }
    if (phase === 'pending') {
      // The destination can mount and signal before the send flow's post-navigate
      // continuation calls end() (await navigate() does not await the commit).
      // Remember it; end() will consume the flag and exit without waiting.
      if (matchesAttached(payload.conversation_id)) {
        revealReceived = true;
      }
    }
    // idle/exiting: reveal is a no-op (direct-link navigation emits it too).
  };

  return {
    getSnapshot: () => ({ phase, pending }),

    begin: (payload) => {
      // Re-entrant begin is a hard reset (defensive; the UI already blocks a
      // second send while a transition is up).
      clearRevealTimer();
      clearExitTimer();
      phase = 'pending';
      pending = { ...payload, stage: 'validating' };
      attachedId = undefined;
      revealReceived = false;
      emit();
    },

    advance: (stage) => {
      if (phase !== 'pending' || !pending) return;
      pending = { ...pending, stage };
      emit();
    },

    attach: (conversationId) => {
      attachedId = conversationId;
    },

    end: () => {
      if (phase !== 'pending') return;
      if (revealReceived) {
        startExit('revealed');
        return;
      }
      phase = 'awaiting-reveal';
      emit();
      revealTimer = setTimeoutFn(() => {
        revealTimer = undefined;
        if (phase === 'awaiting-reveal') {
          startExit('timeout');
        }
      }, revealTimeoutMs);
    },

    abort: () => {
      if (phase !== 'pending' && phase !== 'awaiting-reveal') return;
      clearRevealTimer();
      clearExitTimer();
      onSettled?.('aborted');
      // Synchronous drop: GuidPage is still mounted underneath and the user
      // needs the composer back immediately to retry.
      phase = 'idle';
      pending = null;
      attachedId = undefined;
      emit();
    },

    start: () => {
      if (unsubscribeReveal) return;
      unsubscribeReveal = subscribeReveal(handleReveal);
    },

    stop: () => {
      clearRevealTimer();
      clearExitTimer();
      unsubscribeReveal?.();
      unsubscribeReveal = undefined;
    },
  };
};
