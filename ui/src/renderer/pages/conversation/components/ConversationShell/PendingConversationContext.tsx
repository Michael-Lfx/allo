import type { ConversationId } from '@/common/types/ids';
import { emitter } from '@/renderer/utils/emitter';
import React, { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import {
  createPendingTransitionController,
  type PendingConversation,
  type PendingConversationStage,
  type PendingTransitionController,
  type PendingTransitionPhase,
  type PendingTransitionSnapshot,
} from './pendingTransitionController';

export type { PendingConversation, PendingConversationStage, PendingTransitionPhase };

type PendingConversationContextValue = {
  pending: PendingConversation | null;
  /** Overlay lifecycle phase — the overlay reads it to switch enter/exit animation. */
  phase: PendingTransitionPhase;
  /** Show the loading overlay immediately (called synchronously on send). */
  begin: (payload: PendingConversation) => void;
  /** Advance the overlay only when the corresponding operation actually begins. */
  advance: (stage: PendingConversationStage) => void;
  /** Attach the minted conversation id so the reveal handshake can id-match. */
  attach: (conversationId: ConversationId) => void;
  /**
   * Success path, called after navigate is dispatched. Arms the reveal
   * handshake: the overlay stays up until the destination emits
   * `conversation.transition.reveal` (first bubble committed / page mounted),
   * with a timeout backstop — replacing the old fixed-delay teardown that
   * raced the destination mount and flashed an empty message list.
   */
  end: () => void;
  /** Failure path: drop the overlay instantly (no fade) to restore the Guid composer. */
  abort: () => void;
};

// Stable no-op fallback for consumers rendered outside the provider (e.g. any
// non-shell route). They still get callable handlers so they never null-check.
const NOOP_VALUE: PendingConversationContextValue = {
  pending: null,
  phase: 'idle',
  begin: () => undefined,
  advance: () => undefined,
  attach: () => undefined,
  end: () => undefined,
  abort: () => undefined,
};

const EXIT_DURATION_MS = 140;

const prefersReducedMotion = (): boolean =>
  typeof window !== 'undefined' &&
  typeof window.matchMedia === 'function' &&
  window.matchMedia('(prefers-reduced-motion: reduce)').matches;

const INITIAL_SNAPSHOT: PendingTransitionSnapshot = { phase: 'idle', pending: null };

const PendingConversationContext = createContext<PendingConversationContextValue | null>(null);

/**
 * Hosts the "creating conversation" transition state. Provided at the
 * {@link ConversationShell} level (which wraps the shared `<Outlet/>` and
 * persists across `/guid` ↔ `/conversation/:id`), so the overlay begun on the
 * Guid page stays mounted continuously through the navigation into the real
 * conversation — no fake id, no route juggling.
 *
 * The state machine itself lives in {@link createPendingTransitionController}
 * (React-free, unit-tested); this provider is only the binding: snapshot →
 * state, emitter subscription, reduced-motion exit duration.
 */
export const PendingConversationProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [snapshot, setSnapshot] = useState<PendingTransitionSnapshot>(INITIAL_SNAPSHOT);

  const controllerRef = useRef<PendingTransitionController | null>(null);
  if (!controllerRef.current) {
    controllerRef.current = createPendingTransitionController({
      onChange: setSnapshot,
      exitDurationMs: () => (prefersReducedMotion() ? 0 : EXIT_DURATION_MS),
      subscribeReveal: (fn) => {
        emitter.on('conversation.transition.reveal', fn);
        return () => {
          emitter.off('conversation.transition.reveal', fn);
        };
      },
    });
  }
  const controller = controllerRef.current;

  useEffect(() => {
    controller.start();
    return () => controller.stop();
  }, [controller]);

  const begin = useCallback((payload: PendingConversation) => controller.begin(payload), [controller]);
  const advance = useCallback((stage: PendingConversationStage) => controller.advance(stage), [controller]);
  const attach = useCallback((conversationId: ConversationId) => controller.attach(conversationId), [controller]);
  const end = useCallback(() => controller.end(), [controller]);
  const abort = useCallback(() => controller.abort(), [controller]);

  const value = useMemo<PendingConversationContextValue>(
    () => ({ pending: snapshot.pending, phase: snapshot.phase, begin, advance, attach, end, abort }),
    [snapshot, begin, advance, attach, end, abort]
  );

  return <PendingConversationContext.Provider value={value}>{children}</PendingConversationContext.Provider>;
};

export const usePendingConversation = (): PendingConversationContextValue => {
  return useContext(PendingConversationContext) ?? NOOP_VALUE;
};
