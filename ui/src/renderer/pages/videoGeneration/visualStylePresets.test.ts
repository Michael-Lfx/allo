
import { describe, expect, test } from 'bun:test';
import {
  DEFAULT_VISUAL_STYLE_PROMPT,
  VISUAL_STYLE_PRESETS,
  findVisualStylePreset,
  promptForVisualStyleKey,
  visualStyleSelectValue,
} from './visualStylePresets';

describe('visual style presets', () => {
  test('default cinematic prompt matches backend default look', () => {
    expect(VISUAL_STYLE_PRESETS[0]?.key).toBe('cinematic');
    expect(VISUAL_STYLE_PRESETS[0]?.prompt).toBe(DEFAULT_VISUAL_STYLE_PROMPT);
    expect(visualStyleSelectValue('')).toBe('cinematic');
    expect(promptForVisualStyleKey('cinematic')).toBe(DEFAULT_VISUAL_STYLE_PROMPT);
  });

  test('resolves known prompts and keeps unknown as custom', () => {
    const anime = VISUAL_STYLE_PRESETS.find((preset) => preset.key === 'anime');
    expect(anime).toBeTruthy();
    expect(findVisualStylePreset(anime!.prompt)?.key).toBe('anime');
    expect(visualStyleSelectValue(anime!.prompt)).toBe('anime');
    expect(visualStyleSelectValue('my bespoke neon look')).toBe('__custom__');
  });

  test('stylized presets mention animation cues for backend detection', () => {
    for (const key of ['anime', 'ghibli', 'pixar3d', 'illustration', 'watercolor', 'comic']) {
      const preset = VISUAL_STYLE_PRESETS.find((item) => item.key === key);
      expect(preset).toBeTruthy();
      const lower = preset!.prompt.toLowerCase();
      expect(
        /anime|ghibli|pixar|disney|illustration|watercolor|comic|graphic novel|hand-drawn|painted/.test(
          lower
        )
      ).toBe(true);
    }
  });
});
