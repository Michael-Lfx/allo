/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { FLOWY_BUILTIN_PROVIDER_ID, type ProviderId } from '@/common/types/ids';

/**
 * Candidate voice ids offered by the TTS variant of `TaskModelSelect`.
 *
 * The field is free text on purpose — every provider names its voices
 * differently and new ones ship constantly, so a closed list would go stale and
 * block a voice that works. This table therefore holds ONLY platforms whose
 * voice ids are documented and verified; anything else gets an empty candidate
 * list and the user types the id. Offering guessed ids would be worse than
 * offering none: they look authoritative and fail at synthesis time.
 */
export const TTS_VOICE_OPTIONS_BY_PLATFORM: Record<string, readonly string[]> = {
  openai: ['alloy', 'echo', 'fable', 'onyx', 'nova', 'shimmer'],
  // StepFun (阶跃星辰) system voices, verified against
  // `GET /v1/audio/system_voices?model=step-tts-mini`. Shared across its TTS
  // models (step-tts-mini / step-tts-2 / stepaudio-2.5-tts); still free text, so
  // a cloned or newer voice id can always be typed in.
  stepfun: [
    'cixingnansheng',
    'boyinnansheng',
    'wenrounansheng',
    'shenchennanyin',
    'yuanqinansheng',
    'zhengpaiqingnian',
    'qingniandaxuesheng',
    'wenrougongzi',
    'ruyananshi',
    'jingdiannvsheng',
    'wenrounvsheng',
    'qingchunshaonv',
    'yuanqishaonv',
    'jilingshaonv',
    'tianmeinvsheng',
    'ruanmengnvsheng',
    'linjiajiejie',
    'linjiameimei',
    'zhixingjiejie',
    'shuangkuaijiejie',
    'wenjingxuejie',
    'lengyanyujie',
    'qinqienvsheng',
    'youyanvsheng',
  ],
};

/** Qwen3-TTS voices used by Flowy Cloud `AIPC-qwen3-tts`. */
export const FLOWY_CLOUD_TTS_VOICES: readonly string[] = ['Cherry', 'Serena', 'Ethan', 'Chelsie'];

export const ttsVoiceOptionsFor = (
  platform: string | undefined,
  providerId?: ProviderId | string | null
): readonly string[] => {
  if (
    providerId === FLOWY_BUILTIN_PROVIDER_ID ||
    providerId === 'flowy-cloud'
  ) {
    return FLOWY_CLOUD_TTS_VOICES;
  }
  return (platform && TTS_VOICE_OPTIONS_BY_PLATFORM[platform]) || [];
};
