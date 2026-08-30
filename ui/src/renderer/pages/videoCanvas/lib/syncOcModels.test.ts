/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

import { defaultConfig, selectableModelsByCapability } from '../oc/stores/use-config-store';
import { mergeAlloCatalogIntoConfig } from './syncOcModels';

const source = () => readFileSync(new URL('./syncOcModels.ts', import.meta.url), 'utf8');

describe('mergeAlloCatalogIntoConfig TTS (category=8)', () => {
  test('places Flowy TTS models on the audio capability list', () => {
    const merged = mergeAlloCatalogIntoConfig(defaultConfig, {
      image: [{ id: 'AIPC-seedream', name: 'Seedream' }],
      video: [{ id: 'AIPC-seedance', name: 'Seedance' }],
      audio: [{ id: 'AIPC-qwen3-tts', name: 'qwen3-tts', icon: 'https://example/tts.png' }],
      chat: [],
    });

    expect(merged).not.toBeNull();
    const audioModels = selectableModelsByCapability(merged!, 'audio');
    expect(audioModels).toEqual(['allo-media::AIPC-qwen3-tts']);
    expect(merged!.audioModel).toBe('allo-media::AIPC-qwen3-tts');
    expect(merged!.models).toContain('allo-media::AIPC-qwen3-tts');

    const media = merged!.channels.find((c) => c.id === 'allo-media');
    const ttsCost = media?.modelCosts?.find((item) => item.model === 'AIPC-qwen3-tts');
    expect(ttsCost?.capability).toBe('audio');
    expect(ttsCost?.protocol).toBe('openai-audio');
    expect(ttsCost?.displayName).toBe('qwen3-tts');
    expect(ttsCost?.icon).toBe('https://example/tts.png');
    expect(media?.name).toBe('Flowy Cloud');
  });

  test('labels the media channel Flowy Cloud and keeps catalog icons', () => {
    const merged = mergeAlloCatalogIntoConfig(defaultConfig, {
      image: [
        {
          id: 'AIPC-seedream',
          name: 'Seedream',
          icon: '/static/seedream.png',
        },
      ],
      video: [
        {
          id: 'AIPC-seedance',
          name: 'Seedance',
          icon: 'https://cdn.example/seedance.png',
        },
      ],
      audio: [],
      chat: [],
      serverBaseUrl: 'https://api.flowy.example',
    });

    expect(merged).not.toBeNull();
    const media = merged!.channels.find((c) => c.id === 'allo-media');
    expect(media?.name).toBe('Flowy Cloud');
    expect(media?.modelCosts?.find((item) => item.model === 'AIPC-seedream')?.icon).toBe(
      'https://api.flowy.example/static/seedream.png'
    );
    expect(media?.modelCosts?.find((item) => item.model === 'AIPC-seedance')?.icon).toBe(
      'https://cdn.example/seedance.png'
    );
  });

  test('does not let TTS models leak into image or video pickers', () => {
    const merged = mergeAlloCatalogIntoConfig(defaultConfig, {
      image: [{ id: 'AIPC-seedream', name: 'Seedream' }],
      video: [{ id: 'AIPC-seedance', name: 'Seedance' }],
      audio: [{ id: 'AIPC-qwen3-tts', name: 'qwen3-tts' }],
      chat: [],
    });

    expect(selectableModelsByCapability(merged!, 'image')).toEqual(['allo-media::AIPC-seedream']);
    expect(selectableModelsByCapability(merged!, 'video')).toEqual(['allo-media::AIPC-seedance']);
    expect(selectableModelsByCapability(merged!, 'audio')).not.toContain(
      'allo-media::AIPC-seedream'
    );
  });

  test('syncs audio-only catalogs (no image/video/chat)', () => {
    const merged = mergeAlloCatalogIntoConfig(defaultConfig, {
      image: [],
      video: [],
      audio: [{ id: 'AIPC-qwen3-tts', name: 'qwen3-tts' }],
      chat: [],
    });

    expect(merged).not.toBeNull();
    expect(selectableModelsByCapability(merged!, 'audio')).toEqual(['allo-media::AIPC-qwen3-tts']);
  });

  test('returns null when every catalog is empty', () => {
    expect(
      mergeAlloCatalogIntoConfig(defaultConfig, { image: [], video: [], audio: [], chat: [] })
    ).toBeNull();
  });
});

describe('syncOcModels catalog wiring', () => {
  test('reads audio_models from /api/media/models and maps them as audio', () => {
    const text = source();
    expect(text.includes("audio: mediaList.audio_models || []")).toBe(true);
    expect(text.includes("costEntries(")).toBe(true);
    expect(text.includes("'audio'")).toBe(true);
    expect(text.includes("protocol: 'openai-audio'")).toBe(true);
    expect(text.includes("FLOWY_CLOUD_CHANNEL_NAME")).toBe(true);
    expect(text.includes("rewriteCatalogIconUrl")).toBe(true);
    expect(text.includes("'Allo Media'")).toBe(false);
    expect(text.includes('"Allo Media"')).toBe(false);
  });
});
