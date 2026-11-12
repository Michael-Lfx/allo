/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { configService } from '@/common/config/configService';
import type { SpeechToTextConfig } from '@/common/types/provider/speech';
import { addEventListener, emitter } from '@/renderer/utils/emitter';

export const SPEECH_TO_TEXT_CONFIG_KEY = 'tools.speechToText' as const;

/** Subscribe to same-window changes without allowing a listener to interrupt a save. */
export const subscribeSpeechToTextConfigChanged = (listener: () => void): (() => void) =>
  addEventListener('speech.stt-config.changed', () => {
    try {
      listener();
    } catch (error) {
      if (typeof globalThis.reportError === 'function') {
        globalThis.reportError(error);
      } else {
        console.error('[speechToTextConfig] Listener failed:', error);
      }
    }
  });

export const DEFAULT_SPEECH_TO_TEXT_CONFIG: SpeechToTextConfig = {
  enabled: false,
  provider: 'openai',
  language: '',
};

export const normalizeSpeechToTextConfig = (config?: SpeechToTextConfig): SpeechToTextConfig => {
  if (!config) return DEFAULT_SPEECH_TO_TEXT_CONFIG;

  return {
    ...config,
    provider: config.provider ?? 'openai',
    language:
      config.language ??
      (config.provider === 'openai' ? config.openai?.language : config.provider === 'deepgram' ? config.deepgram?.language : '') ??
      '',
    model:
      config.model ??
      (config.provider === 'openai' ? config.openai?.model : config.provider === 'deepgram' ? config.deepgram?.model : undefined),
  };
};

export const getSpeechToTextConfig = (): SpeechToTextConfig =>
  normalizeSpeechToTextConfig(configService.get(SPEECH_TO_TEXT_CONFIG_KEY));

export const saveSpeechToTextConfig = async (config: SpeechToTextConfig): Promise<void> => {
  const normalized = normalizeSpeechToTextConfig(config);
  try {
    await configService.set(SPEECH_TO_TEXT_CONFIG_KEY, normalized);
  } catch (error) {
    // configService updates its in-memory cache optimistically. Restore the
    // persisted view when the backend rejects the write, so the form and the
    // microphone button do not claim an unsaved provider is enabled.
    await configService.reload();
    throw error;
  } finally {
    if (typeof window !== 'undefined') {
      emitter.emit('speech.stt-config.changed');
    }
  }
};
