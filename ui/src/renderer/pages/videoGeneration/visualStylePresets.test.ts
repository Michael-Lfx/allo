
import { describe, expect, test } from 'bun:test';
import {
  DEFAULT_VISUAL_STYLE_PROMPT,
  VISUAL_STYLE_CATEGORIES,
  VISUAL_STYLE_PRESETS,
  findVisualStylePreset,
  presetsInCategory,
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

  test('catalog is grouped into market-aligned categories', () => {
    expect(VISUAL_STYLE_CATEGORIES.map((c) => c.id)).toEqual([
      'liveAction',
      'genreMood',
      'animation',
      'illustration',
      'crafted',
    ]);
    expect(VISUAL_STYLE_PRESETS.length).toBeGreaterThanOrEqual(24);
    for (const category of VISUAL_STYLE_CATEGORIES) {
      expect(presetsInCategory(category.id).length).toBeGreaterThan(0);
    }
    for (const preset of VISUAL_STYLE_PRESETS) {
      expect(VISUAL_STYLE_CATEGORIES.some((c) => c.id === preset.category)).toBe(true);
      expect(preset.prompt.trim().length).toBeGreaterThan(40);
    }
  });

  test('stylized presets mention animation cues for backend detection', () => {
    const stylizedKeys = [
      'anime',
      'ghibli',
      'donghua',
      'pixar3d',
      'claymation',
      'stopMotion',
      'illustration',
      'watercolor',
      'inkWash',
      'oilPaint',
      'comic',
      'webtoon',
      'pixelArt',
      'blindBox3d',
      'isometric',
    ];
    for (const key of stylizedKeys) {
      const preset = VISUAL_STYLE_PRESETS.find((item) => item.key === key);
      expect(preset).toBeTruthy();
      const lower = preset!.prompt.toLowerCase();
      expect(
        /anime|ghibli|donghua|pixar|disney|illustration|watercolor|ink-wash|oil painting|comic|graphic novel|webtoon|manhwa|hand-drawn|painted|claymation|stop-motion|pixel|chibi|isometric|miniature/.test(
          lower
        )
      ).toBe(true);
    }
  });

  test('live-action genre presets stay photoreal (no animation needles)', () => {
    for (const key of ['cinematic', 'westernEpic', 'horrorGothic', 'sciFiClean', 'editorial']) {
      const preset = VISUAL_STYLE_PRESETS.find((item) => item.key === key);
      expect(preset).toBeTruthy();
      const lower = preset!.prompt.toLowerCase();
      expect(lower.includes('anime')).toBe(false);
      expect(lower.includes('cartoon')).toBe(false);
      expect(lower.includes('pixar')).toBe(false);
    }
  });
});
