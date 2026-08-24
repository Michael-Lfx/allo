import * as bunTest from 'bun:test';
import { describe, expect, test } from 'bun:test';
import { HOME_DRAFT_DEBOUNCE_MS, createHomeDraftWriter } from './homeDraft';

type FakeTimers = {
  useFakeTimers: () => void;
  useRealTimers: () => void;
  clearAllTimers: () => void;
  advanceTimersByTime: (ms: number) => void;
};

const timers = (bunTest as unknown as { jest: FakeTimers }).jest;

describe('home draft writer', () => {
  test('coalesces rapid edits into one debounced write', () => {
    timers.useFakeTimers();
    try {
      let writes = 0;
      const writer = createHomeDraftWriter(() => {
        writes += 1;
      }, HOME_DRAFT_DEBOUNCE_MS);

      writer.markDirty();
      writer.markDirty();
      writer.markDirty();
      timers.advanceTimersByTime(HOME_DRAFT_DEBOUNCE_MS - 1);
      expect(writes).toBe(0);
      timers.advanceTimersByTime(1);
      expect(writes).toBe(1);
    } finally {
      timers.clearAllTimers();
      timers.useRealTimers();
    }
  });

  test('flush writes the latest state immediately, skipping the debounce', () => {
    timers.useFakeTimers();
    try {
      let lastDraft = 0;
      const written: number[] = [];
      const writer = createHomeDraftWriter(() => {
        written.push(lastDraft);
      }, HOME_DRAFT_DEBOUNCE_MS);

      lastDraft = 1;
      writer.markDirty();
      lastDraft = 2;
      writer.flush();
      expect(written).toEqual([2]);
      expect(written).toHaveLength(1);
    } finally {
      timers.clearAllTimers();
      timers.useRealTimers();
    }
  });

  test('dispose writes a pending edit (route change never loses the last change)', () => {
    timers.useFakeTimers();
    try {
      let lastDraft = 0;
      const written: number[] = [];
      const writer = createHomeDraftWriter(() => {
        written.push(lastDraft);
      }, HOME_DRAFT_DEBOUNCE_MS);

      lastDraft = 3;
      writer.markDirty();
      writer.dispose();
      expect(written).toEqual([3]);
    } finally {
      timers.clearAllTimers();
      timers.useRealTimers();
    }
  });

  test('clear cancels the pending write without persisting (submitted draft)', () => {
    timers.useFakeTimers();
    try {
      let writes = 0;
      const writer = createHomeDraftWriter(() => {
        writes += 1;
      }, HOME_DRAFT_DEBOUNCE_MS);

      writer.markDirty();
      writer.clear();
      timers.advanceTimersByTime(HOME_DRAFT_DEBOUNCE_MS * 2);
      expect(writes).toBe(0);
      // A later edit still persists again.
      writer.markDirty();
      timers.advanceTimersByTime(HOME_DRAFT_DEBOUNCE_MS);
      expect(writes).toBe(1);
    } finally {
      timers.clearAllTimers();
      timers.useRealTimers();
    }
  });
});
