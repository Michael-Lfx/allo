/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import * as bunTest from 'bun:test';
import { readFileSync } from 'node:fs';
import { configService } from '@/common/config/configService';
import { TEXT_TO_SPEECH_CONFIG_KEY, saveTextToSpeechConfig, subscribeTextToSpeechConfigChanged } from './textToSpeechConfig';

const { describe, expect, test, spyOn } = bunTest as typeof bunTest & {
  spyOn: (object: typeof configService, method: 'set' | 'reload') => {
    mockResolvedValue: (value: undefined) => { mockRestore: () => void };
    mockRejectedValue: (error: Error) => { mockRestore: () => void };
    mockImplementation: (fn: (...args: unknown[]) => Promise<void>) => { mockRestore: () => void };
  };
};

const src = readFileSync(new URL('./textToSpeechConfig.ts', import.meta.url), 'utf8');
const panel = readFileSync(
  new URL('../pages/modelHub/TextToSpeechContent.tsx', import.meta.url),
  'utf8'
);

describe('tools.textToSpeech client service', () => {
  test('uses the key the backend registered as a Provider reference', () => {
    expect(TEXT_TO_SPEECH_CONFIG_KEY).toBe('tools.textToSpeech');
  });

  test('there is no enabled switch to disagree with the key itself', () => {
    expect(src.includes('enabled')).toBe(false);
  });

  test('clearing the default deletes the key rather than storing a blank object', () => {
    // The backend registers this key as a REQUIRED model reference: an object
    // with an empty provider_id would be rejected at the write boundary, so
    // "no default" has to be expressed as key deletion (null value).
    expect(src.includes('configService.set(TEXT_TO_SPEECH_CONFIG_KEY, undefined)')).toBe(true);
  });

  test('a failed write restores the persisted view instead of lying', () => {
    expect(src.includes('configService.reload()')).toBe(true);
  });

  test('the panel picks the model through the shared TTS selector variant', () => {
    expect(panel.includes('<TaskModelSelect')).toBe(true);
    expect(panel.includes("task='speech_synthesis'")).toBe(true);
    expect(panel.includes('withVoice')).toBe(true);
  });
});

describe('TTS configuration change notifications', () => {
  test('notifies synchronously after successful save and stops after unsubscribe', async () => {
    const windowDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'window');
    Object.defineProperty(globalThis, 'window', { configurable: true, value: {} });
    const writes: unknown[][] = [];
    const set = spyOn(configService, 'set').mockImplementation(async (...args) => { writes.push(args); });
    const calls: string[] = [];
    const unsubscribe = subscribeTextToSpeechConfigChanged(() => calls.push('changed'));
    try {
      await saveTextToSpeechConfig(null);
      expect(writes).toEqual([[TEXT_TO_SPEECH_CONFIG_KEY, undefined]]);
      expect(calls).toEqual(['changed']);
      unsubscribe();
      await saveTextToSpeechConfig(null);
      expect(calls).toEqual(['changed']);
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
    const unsubscribe = subscribeTextToSpeechConfigChanged(() => order.push('changed'));
    try {
      let caught: unknown;
      try {
        await saveTextToSpeechConfig(null);
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

  test('reports a listener failure without blocking later listeners or the save', async () => {
    const windowDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'window');
    const reportDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'reportError');
    Object.defineProperty(globalThis, 'window', { configurable: true, value: {} });
    const errors: unknown[] = [];
    Object.defineProperty(globalThis, 'reportError', { configurable: true, value: (error: unknown) => errors.push(error) });
    const set = spyOn(configService, 'set').mockResolvedValue(undefined);
    const failure = new Error('listener failed');
    const calls: string[] = [];
    const first = subscribeTextToSpeechConfigChanged(() => { throw failure; });
    const second = subscribeTextToSpeechConfigChanged(() => calls.push('changed'));
    try {
      await saveTextToSpeechConfig(null);
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
