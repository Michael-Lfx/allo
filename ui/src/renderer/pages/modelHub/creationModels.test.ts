/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';

import type { IProvider, ModelProfile } from '@/common/config/storage';
import { getCreationModels } from './creationModels';

const provider = (overrides: Partial<IProvider> = {}): IProvider =>
  ({
    id: 'prov_1',
    platform: 'openai',
    name: 'OpenAI',
    base_url: 'https://api.openai.com',
    api_key: 'k',
    models: ['plain-chat', 'dall-e-3'],
    enabled: true,
    ...overrides,
  }) as IProvider;

const profile = (
  source: ModelProfile['source'],
  model: string,
  tasks: ModelProfile['tasks']
): ModelProfile => ({
  provider_id: 'prov_1',
  model,
  tasks,
  traits: [],
  params: {},
  source,
  updated_at: 1,
});

describe('getCreationModels profile precedence', () => {
  test('user profile wins over catalog for the same model', () => {
    const providers = [provider({ models: ['plain-chat'] })];
    const profiles = [
      profile('catalog', 'plain-chat', ['image_generation']),
      profile('user', 'plain-chat', ['video_generation']),
    ];

    const entries = getCreationModels(providers, undefined, profiles);
    expect(entries).toHaveLength(1);
    expect(entries[0].model).toBe('plain-chat');
    expect(entries[0].capabilities).toEqual(['video_generation']);
  });

  test('catalog overrides heuristic so a non-image name is included', () => {
    const providers = [provider({ models: ['plain-chat'] })];
    const profiles = [profile('catalog', 'plain-chat', ['image_generation'])];

    expect(getCreationModels(providers, undefined, undefined)).toEqual([]);
    const entries = getCreationModels(providers, undefined, profiles);
    expect(entries).toHaveLength(1);
    expect(entries[0].capabilities).toEqual(['image_generation']);
  });

  test('inferred does not override heuristic', () => {
    const providers = [provider({ models: ['plain-chat', 'dall-e-3'] })];
    const profiles = [
      profile('inferred', 'plain-chat', ['image_generation']),
      profile('inferred', 'dall-e-3', []),
    ];

    const withoutProfiles = getCreationModels(providers, undefined, undefined);
    const withInferred = getCreationModels(providers, undefined, profiles);

    expect(withoutProfiles.map((e) => e.model)).toEqual(['dall-e-3']);
    expect(withInferred.map((e) => e.model)).toEqual(['dall-e-3']);
    expect(withInferred[0].capabilities).toEqual(['image_generation']);
  });
});
