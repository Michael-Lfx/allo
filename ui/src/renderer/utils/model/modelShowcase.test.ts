/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';

import {
  MODEL_SHOWCASE,
  normalizeShowcaseModelId,
  resolveModelShowcase,
} from './modelShowcase';

describe('normalizeShowcaseModelId', () => {
  test('strips catalog prefixes and normalizes case', () => {
    expect(normalizeShowcaseModelId('AIPC-deepseek-v4.1-flash')).toBe('deepseek-v4.1-flash');
    expect(normalizeShowcaseModelId('aipc-glm-5.2')).toBe('glm-5.2');
    expect(normalizeShowcaseModelId('AIPC-Kimi-K2.5')).toBe('kimi-k2.5');
    expect(normalizeShowcaseModelId('flowy/qwen3.7-max')).toBe('qwen3.7-max');
    expect(normalizeShowcaseModelId('  GLM-5  ')).toBe('glm-5');
  });
});

describe('MODEL_SHOWCASE registry', () => {
  // Lookups run on normalized ids; a key stored in any other form would never
  // match and would silently lose its tagline.
  test('keeps every registry key in normalized form', () => {
    const unnormalized = Object.keys(MODEL_SHOWCASE).filter(
      (key) => normalizeShowcaseModelId(key) !== key
    );
    expect(unnormalized).toEqual([]);
  });
});

describe('resolveModelShowcase', () => {
  test('matches registered models exactly', () => {
    const showcase = resolveModelShowcase('AIPC-glm-5.2');
    expect(showcase.taglineKey).toBe('conversation.modelPicker.tagline.glm-5-2');
    expect(showcase.icon).toContain('/ai-china/zhipu.svg');
    expect(showcase.recommended).toBe(false);
  });

  test('pins exactly one recommended model', () => {
    expect(resolveModelShowcase('AIPC-deepseek-v4.1-flash').recommended).toBe(true);
    const recommendedKeys = Object.entries(MODEL_SHOWCASE)
      .filter(([, entry]) => entry.recommended)
      .map(([key]) => key);
    expect(recommendedKeys).toEqual(['deepseek-v4.1-flash']);
  });

  test('inherits the base tagline on a prefix boundary', () => {
    const variant = resolveModelShowcase('AIPC-deepseek-v4-pro-preview');
    expect(variant.taglineKey).toBe('conversation.modelPicker.tagline.deepseek-v4-pro');
    expect(variant.recommended).toBe(false);
  });

  test('does not inherit across a non-delimiter boundary', () => {
    const showcase = resolveModelShowcase('glm-50');
    expect(showcase.taglineKey).toBeUndefined();
    expect(showcase.recommended).toBe(false);
  });

  test('resolves vendor icons for unregistered models of known brands', () => {
    expect(resolveModelShowcase('deepseek-v9-ultra').icon).toContain('/ai-major/deepseek.svg');
    expect(resolveModelShowcase('kimi-k3').icon).toContain('/ai-china/kimi.svg');
    expect(resolveModelShowcase('minimax-m4').icon).toContain('/ai-china/minimax.png');
    expect(resolveModelShowcase('qwen4-turbo').icon).toContain('/ai-china/qwen.svg');
  });

  test('degrades to an empty showcase for unknown brands', () => {
    const showcase = resolveModelShowcase('unregistered-brand-x1');
    expect(showcase).toEqual({ icon: '', taglineKey: undefined, recommended: false });
  });

  test('keeps auto tiers registered for the tier selector copy', () => {
    expect(resolveModelShowcase('AIPC-auto-intelligence').taglineKey).toBe(
      'conversation.modelPicker.tagline.auto-intelligence'
    );
    expect(resolveModelShowcase('AIPC-auto-balance').taglineKey).toBe(
      'conversation.modelPicker.tagline.auto-balance'
    );
    expect(resolveModelShowcase('AIPC-auto-cost').taglineKey).toBe(
      'conversation.modelPicker.tagline.auto-cost'
    );
  });
});
