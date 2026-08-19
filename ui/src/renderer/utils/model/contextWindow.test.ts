/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, it } from 'vitest';
import {
  DEFAULT_CONTEXT_WINDOW_TOKENS,
  catalogContextLimitForModel,
  resolveDisplayContextWindow,
} from './contextWindow';

describe('catalogContextLimitForModel', () => {
  it('prefers models_detail.context_limit over the provider map', () => {
    const provider = {
      model_context_limits: { 'AIPC-tiny': 32_000 },
      models_detail: [
        {
          provider_id: 'p1',
          model: 'AIPC-tiny',
          enabled: true,
          sort_order: 0,
          tasks: ['chat'],
          traits: [],
          params: {},
          context_limit: 64_000,
          source: 'catalog',
          created_at: 0,
          updated_at: 0,
        },
      ],
    } as const;
    expect(catalogContextLimitForModel(provider as never, 'AIPC-tiny')).toBe(64_000);
  });

  it('falls back to model_context_limits when the row has no limit', () => {
    const provider = {
      model_context_limits: { 'AIPC-tiny': 32_000 },
      models_detail: [
        {
          provider_id: 'p1',
          model: 'AIPC-tiny',
          enabled: true,
          sort_order: 0,
          tasks: ['chat'],
          traits: [],
          params: {},
          source: 'catalog',
          created_at: 0,
          updated_at: 0,
        },
      ],
    } as const;
    expect(catalogContextLimitForModel(provider as never, 'AIPC-tiny')).toBe(32_000);
  });

  it('ignores non-positive limits and unknown models', () => {
    const provider = {
      model_context_limits: { 'AIPC-zero': 0, 'AIPC-tiny': 32_000 },
      models_detail: [
        {
          provider_id: 'p1',
          model: 'AIPC-zero',
          enabled: true,
          sort_order: 0,
          tasks: ['chat'],
          traits: [],
          params: {},
          context_limit: 0,
          source: 'catalog',
          created_at: 0,
          updated_at: 0,
        },
      ],
    } as const;
    expect(catalogContextLimitForModel(provider as never, 'AIPC-zero')).toBeUndefined();
    expect(catalogContextLimitForModel(provider as never, 'missing')).toBeUndefined();
    expect(catalogContextLimitForModel(undefined, 'AIPC-tiny')).toBeUndefined();
  });
});

describe('resolveDisplayContextWindow', () => {
  it('uses the catalog limit when present and otherwise the engine default', () => {
    expect(resolveDisplayContextWindow(200_000)).toBe(200_000);
    expect(resolveDisplayContextWindow(undefined)).toBe(DEFAULT_CONTEXT_WINDOW_TOKENS);
    expect(resolveDisplayContextWindow(0)).toBe(DEFAULT_CONTEXT_WINDOW_TOKENS);
    expect(DEFAULT_CONTEXT_WINDOW_TOKENS).toBe(128_000);
  });
});
