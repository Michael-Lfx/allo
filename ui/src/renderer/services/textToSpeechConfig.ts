/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { configService } from '@/common/config/configService';
import type { TextToSpeechConfig } from '@/common/types/provider/speech';
import { addEventListener, emitter } from '@/renderer/utils/emitter';

export const TEXT_TO_SPEECH_CONFIG_KEY = 'tools.textToSpeech' as const;
/** Subscribe to same-window changes without allowing a listener to interrupt a save. */
export const subscribeTextToSpeechConfigChanged = (listener: () => void): (() => void) =>
  addEventListener('speech.tts-config.changed', () => {
    try {
      listener();
    } catch (error) {
      if (typeof globalThis.reportError === 'function') {
        globalThis.reportError(error);
      } else {
        console.error('[textToSpeechConfig] Listener failed:', error);
      }
    }
  });

/** The install-wide synthesis default, or `undefined` when none is set. */
export const getTextToSpeechConfig = (): TextToSpeechConfig | undefined =>
  configService.get(TEXT_TO_SPEECH_CONFIG_KEY);

/**
 * Persist (or clear) the install-wide synthesis default.
 *
 * `null` DELETES the key. The backend registers this preference as a required
 * `{provider_id, model}` reference, so a blank object would be refused at the
 * write boundary — absence is the only representation of "no default".
 */
export const saveTextToSpeechConfig = async (config: TextToSpeechConfig | null): Promise<void> => {
  try {
    if (config == null) {
      await configService.set(TEXT_TO_SPEECH_CONFIG_KEY, undefined);
      return;
    }
    await configService.set(TEXT_TO_SPEECH_CONFIG_KEY, config);
  } catch (error) {
    // configService updates its in-memory cache optimistically. Restore the
    // persisted view when the backend rejects the write, so the form does not
    // claim a voice is configured when nothing was saved.
    await configService.reload();
    throw error;
  } finally {
    if (typeof window !== 'undefined') {
      emitter.emit('speech.tts-config.changed');
    }
  }
};
