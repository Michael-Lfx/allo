/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import {
  formatTurnCreditDetails,
  formatTurnCreditModelLabel,
  formatTurnCreditModels,
  resolveCatalogModelName,
} from './turnCreditsLabel';

describe('resolveCatalogModelName', () => {
  test('maps billed ids onto AIPC catalog keys used by the model list', () => {
    expect(
      resolveCatalogModelName('deepseek-v4-flash', {
        models: ['AIPC-deepseek-v4-flash'],
        model_descriptions: { 'AIPC-deepseek-v4-flash': 'Deepseek 4.0 Flash' },
      })
    ).toBe('AIPC-deepseek-v4-flash');
  });
});

describe('formatTurnCreditModelLabel', () => {
  test('uses the same catalog description the model list shows', () => {
    expect(
      formatTurnCreditModelLabel('deepseek-v4-flash', {
        models: ['AIPC-deepseek-v4-flash'],
        model_descriptions: { 'AIPC-deepseek-v4-flash': 'Deepseek 4.0 Flash' },
      })
    ).toBe('Deepseek 4.0 Flash');
  });

  test('falls back to the catalog id without inventing title case', () => {
    expect(formatTurnCreditModelLabel('deepseek-v4-flash')).toBe('deepseek-v4-flash');
    expect(formatTurnCreditModelLabel('AIPC-glm-4.7')).toBe('glm-4.7');
  });
});

describe('formatTurnCreditDetails', () => {
  test('coalesces repeated model charges with catalog labels', () => {
    expect(
      formatTurnCreditDetails(
        [
          { modelName: 'deepseek-v4-flash', creditConsumed: 10 },
          { modelName: 'deepseek-v4-flash', creditConsumed: 10 },
        ],
        {
          models: ['AIPC-deepseek-v4-flash'],
          model_descriptions: { 'AIPC-deepseek-v4-flash': 'Deepseek 4.0 Flash' },
        }
      )
    ).toBe('Deepseek 4.0 Flash: 20');
  });
});

describe('formatTurnCreditModels', () => {
  test('joins unique billed models with catalog labels', () => {
    expect(
      formatTurnCreditModels(
        [
          { modelName: 'deepseek-v4-flash', creditConsumed: 20 },
          { modelName: 'glm-4.7', creditConsumed: 8 },
        ],
        undefined,
        {
          models: ['AIPC-deepseek-v4-flash', 'AIPC-glm-4.7'],
          model_descriptions: {
            'AIPC-deepseek-v4-flash': 'Deepseek 4.0 Flash',
            'AIPC-glm-4.7': 'GLM 4.7',
          },
        }
      )
    ).toBe('Deepseek 4.0 Flash, GLM 4.7');
  });

  test('falls back to the conversation model when calls are empty', () => {
    expect(formatTurnCreditModels([], 'deepseek-v4-flash')).toBe('deepseek-v4-flash');
  });
});
