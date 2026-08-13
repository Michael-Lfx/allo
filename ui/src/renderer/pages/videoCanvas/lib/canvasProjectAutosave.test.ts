import * as bunTest from 'bun:test';
import { describe, expect, test } from 'bun:test';
import {
  canvasAutosaveRetryDelayMs,
  createCanvasProjectAutosaveController,
} from './canvasProjectAutosave';

type FakeTimers = {
  useFakeTimers: () => void;
  useRealTimers: () => void;
  clearAllTimers: () => void;
  advanceTimersByTime: (ms: number) => void;
};

const timers = (bunTest as unknown as { jest: FakeTimers }).jest;

const flushPromises = async () => {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
};

describe('canvas project autosave controller', () => {
  test('coalesces edits into one serialized save', async () => {
    timers.useFakeTimers();
    try {
      let calls = 0;
      const controller = createCanvasProjectAutosaveController({
        debounceMs: 10,
        save: async () => {
          calls += 1;
        },
      });

      controller.markDirty();
      controller.markDirty();
      timers.advanceTimersByTime(9);
      await flushPromises();
      expect(calls).toBe(0);
      timers.advanceTimersByTime(1);
      await flushPromises();
      expect(calls).toBe(1);
      await controller.dispose();
    } finally {
      timers.clearAllTimers();
      timers.useRealTimers();
    }
  });

  test('retries a failed save with exponential backoff instead of a hot loop', async () => {
    timers.useFakeTimers();
    try {
      let calls = 0;
      const controller = createCanvasProjectAutosaveController({
        debounceMs: 10,
        retryBaseMs: 20,
        retryMaxMs: 100,
        save: async () => {
          calls += 1;
          if (calls === 1) throw new Error('temporary failure');
        },
      });

      controller.markDirty();
      timers.advanceTimersByTime(10);
      await flushPromises();
      expect(calls).toBe(1);
      timers.advanceTimersByTime(19);
      await flushPromises();
      expect(calls).toBe(1);
      timers.advanceTimersByTime(1);
      await flushPromises();
      expect(calls).toBe(2);
      expect(canvasAutosaveRetryDelayMs(1)).toBe(1_000);
      expect(canvasAutosaveRetryDelayMs(8)).toBe(30_000);
      await controller.dispose();
    } finally {
      timers.clearAllTimers();
      timers.useRealTimers();
    }
  });

  test('flushes the newest dirty state after an in-flight save during dispose', async () => {
    timers.useFakeTimers();
    try {
      let calls = 0;
      let resolveFirst!: () => void;
      const firstSave = new Promise<void>((resolve) => {
        resolveFirst = resolve;
      });
      const controller = createCanvasProjectAutosaveController({
        debounceMs: 10,
        save: async () => {
          calls += 1;
          if (calls === 1) await firstSave;
        },
      });

      controller.markDirty();
      timers.advanceTimersByTime(10);
      await flushPromises();
      expect(calls).toBe(1);

      controller.markDirty();
      const disposed = controller.dispose();
      resolveFirst();
      await disposed;
      expect(calls).toBe(2);
    } finally {
      timers.clearAllTimers();
      timers.useRealTimers();
    }
  });

  test('flush waits for a follow-up save when an edit arrives during an in-flight PUT', async () => {
    timers.useFakeTimers();
    try {
      let calls = 0;
      let resolveFirst!: () => void;
      const firstSave = new Promise<void>((resolve) => {
        resolveFirst = resolve;
      });
      const controller = createCanvasProjectAutosaveController({
        debounceMs: 10,
        save: async () => {
          calls += 1;
          if (calls === 1) await firstSave;
        },
      });

      controller.markDirty();
      timers.advanceTimersByTime(10);
      await flushPromises();
      expect(calls).toBe(1);

      controller.markDirty();
      const flushed = controller.flush();
      resolveFirst();
      await flushed;
      expect(calls).toBe(2);
      await controller.dispose();
    } finally {
      timers.clearAllTimers();
      timers.useRealTimers();
    }
  });
});
