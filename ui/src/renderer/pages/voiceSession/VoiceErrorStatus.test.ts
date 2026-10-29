import { expect, test } from 'bun:test';
import { voiceErrorKey, voiceErrorTranslationKey } from './VoiceErrorStatus';

test('a missing packaged voice component maps to the bilingual repair hint', () => {
  expect(voiceErrorKey('voice_companion_missing')).toBe('voice.errors.componentMissing');
  expect(voiceErrorTranslationKey('voice_companion_missing')).toBe('voice.errors.componentMissing');
});

test('unknown backend errors keep their raw text instead of a generic fallback', () => {
  expect(voiceErrorTranslationKey('not_a_known_code')).toBeNull();
  expect(voiceErrorKey('not_a_known_code')).toBe('voice.session.unavailable');
});
