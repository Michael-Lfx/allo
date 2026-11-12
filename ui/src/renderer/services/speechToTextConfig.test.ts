/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */
import * as bunTest from 'bun:test';
import { configService } from '@/common/config/configService';
import {
  DEFAULT_SPEECH_TO_TEXT_CONFIG,
  SPEECH_TO_TEXT_CONFIG_KEY,
  saveSpeechToTextConfig,
  subscribeSpeechToTextConfigChanged,
} from './speechToTextConfig';

const { describe, expect, test, spyOn } = bunTest as typeof bunTest & {
  spyOn: (object: typeof configService, method: 'set' | 'reload') => {
    mockResolvedValue: (value: undefined) => { mockRestore: () => void };
    mockRejectedValue: (error: Error) => { mockRestore: () => void };
    mockImplementation: (fn: (...args: unknown[]) => Promise<void>) => { mockRestore: () => void };
  };
};

describe('STT configuration change notifications', () => {
  test('saves the normalized config, notifies after saving, and stops after unsubscribe', async () => {
    const windowDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'window');
    Object.defineProperty(globalThis, 'window', { configurable: true, value: {} });
    const writes: unknown[][] = [];
    const order: string[] = [];
    const set = spyOn(configService, 'set').mockImplementation(async (...args) => {
      writes.push(args);
      order.push('saved');
    });
    const unsubscribe = subscribeSpeechToTextConfigChanged(() => order.push('changed'));
    try {
      await saveSpeechToTextConfig(DEFAULT_SPEECH_TO_TEXT_CONFIG);
      expect(writes).toEqual([[SPEECH_TO_TEXT_CONFIG_KEY, { ...DEFAULT_SPEECH_TO_TEXT_CONFIG, model: undefined }]]);
      expect(order).toEqual(['saved', 'changed']);
      unsubscribe();
      await saveSpeechToTextConfig(DEFAULT_SPEECH_TO_TEXT_CONFIG);
      expect(order).toEqual(['saved', 'changed', 'saved']);
    } finally {
      unsubscribe();
      set.mockRestore();
      if (windowDescriptor) Object.defineProperty(globalThis, 'window', windowDescriptor);
      else Reflect.deleteProperty(globalThis, 'window');
    }
  });

  test('reloads before notifying on failure and preserves the save error', async () => {
    const windowDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'window');
    Object.defineProperty(globalThis, 'window', { configurable: true, value: {} });
    const failure = new Error('save failed');
    const order: string[] = [];
    const set = spyOn(configService, 'set').mockRejectedValue(failure);
    const reload = spyOn(configService, 'reload').mockImplementation(async () => { order.push('reload'); });
    const unsubscribe = subscribeSpeechToTextConfigChanged(() => order.push('changed'));
    try {
      let caught: unknown;
      try {
        await saveSpeechToTextConfig(DEFAULT_SPEECH_TO_TEXT_CONFIG);
      } catch (error) {
        caught = error;
      }
      expect(caught).toBe(failure);
      expect(order).toEqual(['reload', 'changed']);
    } finally {
      unsubscribe();
      set.mockRestore();
      reload.mockRestore();
      if (windowDescriptor) Object.defineProperty(globalThis, 'window', windowDescriptor);
      else Reflect.deleteProperty(globalThis, 'window');
    }
  });

  test('reports listener failure without blocking later listeners or the save', async () => {
    const windowDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'window');
    const reportDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'reportError');
    Object.defineProperty(globalThis, 'window', { configurable: true, value: {} });
    const errors: unknown[] = [];
    Object.defineProperty(globalThis, 'reportError', { configurable: true, value: (error: unknown) => errors.push(error) });
    const set = spyOn(configService, 'set').mockResolvedValue(undefined);
    const failure = new Error('listener failed');
    const calls: string[] = [];
    const first = subscribeSpeechToTextConfigChanged(() => { throw failure; });
    const second = subscribeSpeechToTextConfigChanged(() => calls.push('changed'));
    try {
      await saveSpeechToTextConfig(DEFAULT_SPEECH_TO_TEXT_CONFIG);
      expect(errors).toEqual([failure]);
      expect(calls).toEqual(['changed']);
    } finally {
      first();
      second();
      set.mockRestore();
      if (windowDescriptor) Object.defineProperty(globalThis, 'window', windowDescriptor);
      else Reflect.deleteProperty(globalThis, 'window');
      if (reportDescriptor) Object.defineProperty(globalThis, 'reportError', reportDescriptor);
      else Reflect.deleteProperty(globalThis, 'reportError');
    }
  });
});
