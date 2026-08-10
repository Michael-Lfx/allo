/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, it } from 'vitest';
import {
  FLOWY_CATALOG_REASONING_EFFORT_PARAM,
  catalogReasoningEffortForModel,
  defaultReasoningEffort,
  parseCatalogReasoningEffortLevels,
  resolveReasoningEffortForSelection,
} from './reasoningEffort';

describe('reasoningEffort helpers', () => {
  it('parses catalog-owned levels and drops empties/duplicates', () => {
    expect(
      parseCatalogReasoningEffortLevels({
        [FLOWY_CATALOG_REASONING_EFFORT_PARAM]: ['low', ' medium ', 'xhigh', 'low', '', 1],
      })
    ).toEqual(['low', 'medium', 'xhigh']);
    expect(parseCatalogReasoningEffortLevels({})).toEqual([]);
    expect(parseCatalogReasoningEffortLevels(null)).toEqual([]);
  });

  it('defaults to medium when present', () => {
    expect(defaultReasoningEffort(['low', 'medium', 'xhigh'])).toBe('medium');
    expect(defaultReasoningEffort(['low', 'xhigh'])).toBe('low');
    expect(defaultReasoningEffort([])).toBeUndefined();
  });

  it('reads levels from provider models_detail params', () => {
    const provider = {
      models_detail: [
        {
          provider_id: 'p1',
          model: 'AIPC-think',
          enabled: true,
          sort_order: 0,
          tasks: ['chat'],
          traits: ['reasoning'],
          params: {
            [FLOWY_CATALOG_REASONING_EFFORT_PARAM]: ['low', 'medium', 'xhigh'],
          },
          source: 'inferred',
          created_at: 0,
          updated_at: 0,
        },
      ],
    } as const;
    expect(catalogReasoningEffortForModel(provider as never, 'AIPC-think')).toEqual([
      'low',
      'medium',
      'xhigh',
    ]);
    expect(catalogReasoningEffortForModel(provider as never, 'missing')).toEqual([]);
  });

  it('resolves preferred effort against catalog allowlist', () => {
    const selection = {
      id: 'p1',
      use_model: 'AIPC-think',
      models_detail: [
        {
          provider_id: 'p1',
          model: 'AIPC-think',
          enabled: true,
          sort_order: 0,
          tasks: ['chat'],
          traits: ['reasoning'],
          params: {
            [FLOWY_CATALOG_REASONING_EFFORT_PARAM]: ['low', 'medium', 'xhigh'],
          },
          source: 'inferred',
          created_at: 0,
          updated_at: 0,
        },
      ],
    } as never;
    expect(resolveReasoningEffortForSelection(selection, 'xhigh').effort).toBe('xhigh');
    expect(resolveReasoningEffortForSelection(selection, 'high').effort).toBe('medium');
    expect(resolveReasoningEffortForSelection(selection, undefined).effort).toBe('medium');
  });
});
