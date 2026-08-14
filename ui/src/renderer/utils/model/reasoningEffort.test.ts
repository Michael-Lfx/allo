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
  reasoningEffortAtIndex,
  reasoningEffortIndex,
  reasoningEffortProgress,
  reasoningEffortSliderViewModel,
  parseCatalogReasoningEffortLevels,
  pendingReasoningEffortCommitIndex,
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

  it('keeps Cloud order as the discrete shallow-to-deep slider order', () => {
    const view = reasoningEffortSliderViewModel(['low', 'medium', 'high', 'xhigh'], 'high');

    expect(view.levels).toEqual(['low', 'medium', 'high', 'xhigh']);
    expect(view.index).toBe(2);
    expect(view.effort).toBe('high');
    expect(view.progress).toBe(2 / 3);
    expect(view.isStatic).toBe(false);
  });

  it('uses the returned levels and count without inserting or alphabetizing values', () => {
    const levels = ['ultra', 'low', 'xhigh', 'low', ''];
    const view = reasoningEffortSliderViewModel(levels, 'missing');

    expect(view.levels).toEqual(['ultra', 'low', 'xhigh']);
    expect(view.index).toBe(0);
    expect(view.progress).toBe(0);
    expect(reasoningEffortAtIndex(view.levels, 2)).toBe('xhigh');
    expect(reasoningEffortProgress(1, view.levels.length)).toBe(0.5);
  });

  it('maps each actual level count to its own discrete positions', () => {
    expect(reasoningEffortSliderViewModel(['low', 'high'], 'high').progress).toBe(1);
    expect(reasoningEffortSliderViewModel(['low', 'medium', 'high'], 'medium').progress).toBe(0.5);
    expect(reasoningEffortSliderViewModel(['low', 'medium', 'high', 'xhigh'], 'high').progress).toBe(2 / 3);
    expect(
      reasoningEffortSliderViewModel(['minimal', 'low', 'medium', 'high', 'xhigh'], 'high').progress
    ).toBe(0.75);
  });

  it('heals an invalid effort to medium, then to the first catalog level', () => {
    expect(reasoningEffortIndex(['low', 'medium', 'high'], 'retired')).toBe(1);
    expect(reasoningEffortIndex(['low', 'high'], 'retired')).toBe(0);
    expect(reasoningEffortAtIndex(['low', 'medium', 'high'], 99)).toBe('high');
    expect(reasoningEffortAtIndex(['low', 'medium', 'high'], -2)).toBe('low');
  });

  it('renders a single-level catalog as a full static state', () => {
    expect(reasoningEffortSliderViewModel(['medium'], 'high')).toEqual({
      levels: ['medium'],
      effort: 'medium',
      index: 0,
      progress: 1,
      isStatic: true,
    });
    expect(reasoningEffortProgress(-1, 0)).toBe(0);
  });

  it('keeps a compensating choice queued while an earlier save is in flight', () => {
    expect(pendingReasoningEffortCommitIndex(1, 1, true)).toBe(1);
    expect(pendingReasoningEffortCommitIndex(2, 1, true)).toBe(2);
    expect(pendingReasoningEffortCommitIndex(1, 1, false)).toBeNull();
    expect(pendingReasoningEffortCommitIndex(2, 1, false)).toBe(2);
  });
});
