import { describe, expect, test } from 'bun:test';
import { resolveChatModelTriggerPlacement } from './useChatModelTriggerExpansion';

describe('chat model trigger expansion placement', () => {
  test('uses the available right side when the preferred left overlay would clip the icon', () => {
    expect(
      resolveChatModelTriggerPlacement({
        slotRect: { left: 80, right: 108 },
        boundary: { left: 12, right: 348 },
        desiredWidth: 176,
      }),
    ).toEqual({ side: 'end', width: 176 });
  });

  test('keeps the existing leftward placement when it fits', () => {
    expect(
      resolveChatModelTriggerPlacement({
        slotRect: { left: 240, right: 268 },
        boundary: { left: 12, right: 348 },
        desiredWidth: 176,
      }),
    ).toEqual({ side: 'start', width: 176 });
  });

  test('clamps the overlay width when neither side has the requested room', () => {
    expect(
      resolveChatModelTriggerPlacement({
        slotRect: { left: 120, right: 148 },
        boundary: { left: 12, right: 228 },
        desiredWidth: 176,
      }),
    ).toEqual({ side: 'start', width: 136 });
  });

  test('respects logical start/end in RTL layouts', () => {
    expect(
      resolveChatModelTriggerPlacement({
        slotRect: { left: 252, right: 280 },
        boundary: { left: 12, right: 348 },
        desiredWidth: 176,
        direction: 'rtl',
      }),
    ).toEqual({ side: 'end', width: 176 });
  });
});
