import { describe, expect, test } from 'bun:test';
import type { ConversationId } from '@/common/types/ids';
import {
  createPendingTransitionController,
  type PendingTransitionOutcome,
  type PendingTransitionSnapshot,
} from './pendingTransitionController';

type RevealHandler = (payload: { conversation_id: ConversationId }) => void;

/** Manual clock + reveal bus so the state machine is tested without real timers. */
const createHarness = (options?: { revealTimeoutMs?: number; exitDurationMs?: number }) => {
  const snapshots: PendingTransitionSnapshot[] = [];
  const outcomes: PendingTransitionOutcome[] = [];
  const timers: Array<{ fn: () => void; ms: number; cleared: boolean }> = [];
  let revealHandler: RevealHandler | undefined;

  const controller = createPendingTransitionController({
    onChange: (snapshot) => snapshots.push(snapshot),
    onSettled: (outcome) => outcomes.push(outcome),
    revealTimeoutMs: options?.revealTimeoutMs ?? 1500,
    exitDurationMs: () => options?.exitDurationMs ?? 140,
    subscribeReveal: (fn) => {
      revealHandler = fn;
      return () => {
        revealHandler = undefined;
      };
    },
    setTimeoutFn: (fn, ms) => {
      const timer = { fn, ms, cleared: false };
      timers.push(timer);
      return timer;
    },
    clearTimeoutFn: (handle) => {
      (handle as { cleared: boolean }).cleared = true;
    },
  });

  return {
    controller,
    snapshots,
    outcomes,
    timers,
    reveal: (conversation_id: ConversationId) => revealHandler?.({ conversation_id }),
    /** Fire every scheduled timer whose delay matches (and that wasn't cleared). */
    runTimers: (ms: number) => {
      for (const timer of [...timers]) {
        if (!timer.cleared && timer.ms === ms) timer.fn();
      }
    },
    lastPhase: () => snapshots[snapshots.length - 1]?.phase,
  };
};

const ID_A = 'conv-a' as ConversationId;
const ID_B = 'conv-b' as ConversationId;
const PAYLOAD = { input: 'hello', sendsInitialMessage: true };

describe('pendingTransitionController', () => {
  test('begin → advance → attach → end waits in awaiting-reveal', () => {
    const h = createHarness();
    h.controller.start();

    h.controller.begin(PAYLOAD);
    expect(h.lastPhase()).toBe('pending');
    expect(h.snapshots[0].pending?.stage).toBe('validating');

    h.controller.advance('creating');
    expect(h.controller.getSnapshot().pending?.stage).toBe('creating');

    h.controller.attach(ID_A);
    h.controller.end();
    expect(h.lastPhase()).toBe('awaiting-reveal');
    // Overlay content stays up while waiting.
    expect(h.controller.getSnapshot().pending).not.toBeNull();
  });

  test('reveal after end() exits with fade, then drops to idle', () => {
    const h = createHarness();
    h.controller.start();
    h.controller.begin(PAYLOAD);
    h.controller.attach(ID_A);
    h.controller.end();

    h.reveal(ID_A);
    expect(h.lastPhase()).toBe('exiting');
    expect(h.controller.getSnapshot().pending).not.toBeNull();
    expect(h.outcomes).toEqual(['revealed']);

    h.runTimers(140);
    expect(h.lastPhase()).toBe('idle');
    expect(h.controller.getSnapshot().pending).toBeNull();
  });

  test('reveal before end() is remembered; end() exits without waiting', () => {
    const h = createHarness();
    h.controller.start();
    h.controller.begin(PAYLOAD);
    h.controller.attach(ID_A);

    h.reveal(ID_A);
    expect(h.lastPhase()).toBe('pending');

    h.controller.end();
    expect(h.lastPhase()).toBe('exiting');
    expect(h.outcomes).toEqual(['revealed']);
  });

  test('reveal with a mismatched id is ignored; timeout is the backstop', () => {
    const h = createHarness();
    h.controller.start();
    h.controller.begin(PAYLOAD);
    h.controller.attach(ID_A);
    h.controller.end();

    h.reveal(ID_B);
    expect(h.lastPhase()).toBe('awaiting-reveal');

    h.runTimers(1500);
    expect(h.lastPhase()).toBe('exiting');
    expect(h.outcomes).toEqual(['timeout']);
  });

  test('reveal while idle is a no-op (direct-link navigation)', () => {
    const h = createHarness();
    h.controller.start();
    h.reveal(ID_A);
    expect(h.snapshots).toHaveLength(0);
    expect(h.controller.getSnapshot().phase).toBe('idle');
  });

  test('abort drops the overlay synchronously with no fade', () => {
    const h = createHarness();
    h.controller.start();
    h.controller.begin(PAYLOAD);
    h.controller.attach(ID_A);

    h.controller.abort();
    expect(h.lastPhase()).toBe('idle');
    expect(h.controller.getSnapshot().pending).toBeNull();
    expect(h.outcomes).toEqual(['aborted']);
    // No exit timer scheduled — the overlay is already gone.
    expect(h.timers.some((t) => t.ms === 140 && !t.cleared)).toBe(false);
  });

  test('abort from awaiting-reveal clears the pending timeout', () => {
    const h = createHarness();
    h.controller.start();
    h.controller.begin(PAYLOAD);
    h.controller.end();
    h.controller.abort();

    expect(h.lastPhase()).toBe('idle');
    h.runTimers(1500);
    expect(h.lastPhase()).toBe('idle');
    expect(h.outcomes).toEqual(['aborted']);
  });

  test('re-entrant begin resets a live transition', () => {
    const h = createHarness();
    h.controller.start();
    h.controller.begin(PAYLOAD);
    h.controller.end();

    h.controller.begin({ input: 'again', sendsInitialMessage: true });
    expect(h.lastPhase()).toBe('pending');
    expect(h.controller.getSnapshot().pending?.input).toBe('again');

    h.runTimers(1500);
    expect(h.lastPhase()).toBe('pending');
  });

  test('end() without begin is a no-op', () => {
    const h = createHarness();
    h.controller.start();
    h.controller.end();
    expect(h.snapshots).toHaveLength(0);
  });

  test('reveal without attach matches any conversation', () => {
    const h = createHarness();
    h.controller.start();
    h.controller.begin(PAYLOAD);
    h.controller.end();

    h.reveal(ID_B);
    expect(h.lastPhase()).toBe('exiting');
    expect(h.outcomes).toEqual(['revealed']);
  });

  test('zero exit duration settles to idle on the next timer tick', () => {
    const h = createHarness({ exitDurationMs: 0 });
    h.controller.start();
    h.controller.begin(PAYLOAD);
    h.controller.end();
    h.reveal(ID_A);

    expect(h.lastPhase()).toBe('exiting');
    h.runTimers(0);
    expect(h.lastPhase()).toBe('idle');
  });

  test('stop() unsubscribes and clears timers', () => {
    const h = createHarness();
    h.controller.start();
    h.controller.begin(PAYLOAD);
    h.controller.end();
    h.controller.stop();

    h.reveal(ID_A);
    expect(h.lastPhase()).toBe('awaiting-reveal');
    h.runTimers(1500);
    expect(h.lastPhase()).toBe('awaiting-reveal');
  });
});
