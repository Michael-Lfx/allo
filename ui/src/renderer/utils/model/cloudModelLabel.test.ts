/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import {
  formatCloudModelLabel,
  formatModelLabelForProvider,
  hydrateProviderWithModel,
} from './cloudModelLabel';
import type { IProvider, TProviderWithModel } from '@/common/config/storage';

describe('formatCloudModelLabel', () => {
  test('strips AIPC- prefix from raw model id', () => {
    expect(formatCloudModelLabel('AIPC-glm-4.7')).toBe('glm-4.7');
  });

  test('strips AIPC- prefix from catalog descriptions', () => {
    expect(formatCloudModelLabel('AIPC-glm-4.7', { 'AIPC-glm-4.7': 'AIPC-glm-4.7' })).toBe('glm-4.7');
  });

  test('prefers description over raw id', () => {
    expect(formatCloudModelLabel('AIPC-glm-4.7', { 'AIPC-glm-4.7': 'GLM 4.7' })).toBe('GLM 4.7');
  });
});

describe('hydrateProviderWithModel', () => {
  const providers: IProvider[] = [
    {
      id: 'flowy-cloud',
      platform: 'openai',
      name: 'Flowy Cloud',
      base_url: 'https://example.com',
      api_key: 'token',
      models: ['AIPC-glm-4.7'],
      model_descriptions: { 'AIPC-glm-4.7': 'glm-4.7' },
    },
  ];

  test('merges persisted conversation model with live provider catalog', () => {
    const persisted = { id: 'flowy-cloud', use_model: 'AIPC-glm-4.7' } as TProviderWithModel;
    const hydrated = hydrateProviderWithModel(providers, persisted);
    expect(hydrated?.model_descriptions?.['AIPC-glm-4.7']).toBe('glm-4.7');
    expect(hydrated?.use_model).toBe('AIPC-glm-4.7');
  });
});

describe('formatModelLabelForProvider', () => {
  test('formats using provider descriptions when available', () => {
    const provider = {
      model_descriptions: { 'AIPC-glm-4.7': 'GLM 4.7' },
    };
    expect(formatModelLabelForProvider(provider, 'AIPC-glm-4.7')).toBe('GLM 4.7');
  });

  test('falls back to stripping prefix when descriptions are missing', () => {
    expect(formatModelLabelForProvider(undefined, 'AIPC-glm-4.7')).toBe('glm-4.7');
  });
});
