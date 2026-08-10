import { describe, expect, test } from 'bun:test';

import { createComposerStopHandoffGate } from '@/renderer/components/chat/SendBox/composerStopHandoffGate';

describe('createComposerStopHandoffGate', () => {
  test('blocks the second click in a send-to-stop double-click handoff', () => {
    let now = 1_000;
    const gate = createComposerStopHandoffGate(() => now);

    gate.armAfterEditSubmit();
    now = 1_100;

    expect(gate.shouldIgnoreStop(2)).toBe(true);
  });

  test('allows an intentional pointer or keyboard stop inside the handoff window', () => {
    let now = 1_000;
    const gate = createComposerStopHandoffGate(() => now);

    gate.armAfterEditSubmit();
    now = 1_100;

    expect(gate.shouldIgnoreStop(1)).toBe(false);
    expect(gate.shouldIgnoreStop(0)).toBe(false);
  });

  test('allows a multi-click stop after the handoff window', () => {
    let now = 1_000;
    const gate = createComposerStopHandoffGate(() => now);

    gate.armAfterEditSubmit();
    now = 1_500;

    expect(gate.shouldIgnoreStop(2)).toBe(false);
  });
});
