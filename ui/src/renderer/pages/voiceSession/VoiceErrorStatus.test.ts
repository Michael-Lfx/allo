import { expect, test } from 'bun:test';
import { voiceErrorKey, voiceErrorTranslationKey } from './VoiceErrorStatus';

test('a missing packaged voice component maps to the bilingual repair hint', () => {
  expect(voiceErrorKey('voice_companion_missing')).toBe('voice.errors.componentMissing');
  expect(voiceErrorTranslationKey('voice_companion_missing')).toBe('voice.errors.componentMissing');
});

test('a source checkout maps to the build-command hint instead of the installer hint', () => {
  expect(voiceErrorKey('voice_companion_missing_dev')).toBe('voice.errors.componentMissingDev');
  expect(voiceErrorTranslationKey('voice_companion_missing_dev')).toBe('voice.errors.componentMissingDev');
});

test('unknown backend errors keep their raw text instead of a generic fallback', () => {
  expect(voiceErrorTranslationKey('not_a_known_code')).toBeNull();
  expect(voiceErrorKey('not_a_known_code')).toBe('voice.session.unavailable');
});
