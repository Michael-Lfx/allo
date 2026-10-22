/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import zhNomi from '@/renderer/services/i18n/locales/zh-CN/nomi.json';
import enNomi from '@/renderer/services/i18n/locales/en-US/nomi.json';

const src = readFileSync(new URL('./ModelsSection.tsx', import.meta.url), 'utf8');

const ESSENTIAL_SLOT_KEYS = [
  'mainChatModel',
  'ttsSlot',
  'ttsSlotHint',
  'ttsFallback',
] as const;

describe('总览 model slots (streamlined)', () => {
  test('renders essential companion slots (chat model and speech voice)', () => {
    expect(src.includes('nomi.overview.mainChatModel')).toBe(true);
    expect(src.includes('nomi.overview.ttsSlot')).toBe(true);
  });

  test('hides technical clutter (VAD numeric inputs, separate vision slot, separate ASR, and fallback)', () => {
    // VAD tuning removed from overview: uses system defaults
    expect(src.includes('vadSlot')).toBe(false);
    expect(src.includes('NomiInputNumber')).toBe(false);
    // Standalone vision model removed: natively follows conversation context
    expect(src.includes('visionSlot')).toBe(false);
    // Standalone ASR model removed: managed by unified voice pipeline
    expect(src.includes('asrSlot')).toBe(false);
    // Cold disaster fallback removed from overview
    expect(src.includes('fallbackChatModel')).toBe(false);
  });

  test('enables hideProvider to remove repetitive Flowy Cloud provider dropdowns', () => {
    expect(src.includes('hideProvider')).toBe(true);
  });

  test('configures TTS for onlyVoice selection to choose voice directly without model noise', () => {
    expect(src.includes('withVoice')).toBe(true);
    expect(src.includes('onlyVoice')).toBe(true);
  });

  test('copy exists in both locales', () => {
    const overview = (locale: Record<string, unknown>) =>
      (locale as { overview: Record<string, string> }).overview;
    for (const [name, locale] of [
      ['zh-CN', zhNomi as unknown as Record<string, unknown>],
      ['en-US', enNomi as unknown as Record<string, unknown>],
    ] as const) {
      for (const key of ESSENTIAL_SLOT_KEYS) {
        expect(typeof overview(locale)[key]).toBe('string');
        expect(overview(locale)[key].trim().length > 0).toBe(true);
      }
      expect(name.length > 0).toBe(true);
    }
  });
});
