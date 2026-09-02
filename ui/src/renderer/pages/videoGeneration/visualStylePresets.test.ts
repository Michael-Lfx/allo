
import { describe, expect, test } from 'bun:test';
import {
  DEFAULT_VISUAL_STYLE_PROMPT,
  HUOBAO_LOOK_KEYS,
  VISUAL_STYLE_CATEGORIES,
  VISUAL_STYLE_PRESETS,
  findVisualStylePreset,
  hasSelectedVisualStyle,
  isDefaultVisualStyle,
  presetsInCategory,
  promptForVisualStyleKey,
  visualStyleSelectValue,
} from './visualStylePresets';

describe('visual style presets', () => {
  test('default cinematic prompt matches backend default look', () => {
    expect(VISUAL_STYLE_PRESETS[0]?.key).toBe('cinematic');
    expect(VISUAL_STYLE_PRESETS[0]?.prompt).toBe(DEFAULT_VISUAL_STYLE_PROMPT);
    expect(visualStyleSelectValue('')).toBe('');
    expect(findVisualStylePreset('')).toBeUndefined();
    expect(promptForVisualStyleKey('cinematic')).toBe(DEFAULT_VISUAL_STYLE_PROMPT);
  });

  test('resolves known prompts and keeps unknown as custom', () => {
    const anime = VISUAL_STYLE_PRESETS.find((preset) => preset.key === 'anime');
    expect(anime).toBeTruthy();
    expect(findVisualStylePreset(anime!.prompt)?.key).toBe('anime');
    expect(visualStyleSelectValue(anime!.prompt)).toBe('anime');
    expect(visualStyleSelectValue('my bespoke neon look')).toBe('__custom__');
    expect(isDefaultVisualStyle('')).toBe(true);
    expect(hasSelectedVisualStyle('')).toBe(false);
    expect(isDefaultVisualStyle(DEFAULT_VISUAL_STYLE_PROMPT)).toBe(false);
    expect(hasSelectedVisualStyle(DEFAULT_VISUAL_STYLE_PROMPT)).toBe(true);
    expect(isDefaultVisualStyle(promptForVisualStyleKey('manhua3d'))).toBe(false);
  });

  test('migrates huobao-drama style seeds that were missing from the catalog', () => {
    for (const key of HUOBAO_LOOK_KEYS) {
      const preset = VISUAL_STYLE_PRESETS.find((item) => item.key === key);
      expect(preset).toBeTruthy();
    }
    expect(promptForVisualStyleKey('manhua3d')).toContain('3D CG animation style');
    expect(promptForVisualStyleKey('manhua3d')).toContain('game-engine quality render');
    expect(promptForVisualStyleKey('animeCel')).toContain('cel shading');
    expect(promptForVisualStyleKey('animeCel')).toContain('Japanese anime style');
  });

  test('catalog is grouped into market-aligned categories', () => {
    expect(VISUAL_STYLE_CATEGORIES.map((c) => c.id)).toEqual([
      'liveAction',
      'genreMood',
      'animation',
      'illustration',
      'crafted',
    ]);
    expect(VISUAL_STYLE_PRESETS.length).toBeGreaterThanOrEqual(70);
    const keys = VISUAL_STYLE_PRESETS.map((preset) => preset.key);
    const prompts = VISUAL_STYLE_PRESETS.map((preset) => preset.prompt);
    expect(new Set(keys).size).toBe(keys.length);
    expect(new Set(prompts).size).toBe(prompts.length);
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
      'animeCel',
      'ghibli',
      'shinkai',
      'donghua',
      'pixar3d',
      'manhua3d',
      'claymation',
      'stopMotion',
      'americanCartoon',
      'lowPoly',
      'illustration',
      'watercolor',
      'inkWash',
      'oilPaint',
      'comic',
      'webtoon',
      'ukiyoE',
      'paperCut',
      'charcoal',
      'lineArt',
      'dunhuang',
      'stainedGlass',
      'artNouveau',
      'popArt',
      'shadowPuppet',
      'pastelGouache',
      'shoujoManga',
      'lianhuanhua',
      'pixelArt',
      'blindBox3d',
      'isometric',
      'toonShader3d',
      'unrealCinematic',
      'disney2d',
      'chibi',
      'mechaAnime',
      'legoBrickfilm',
      'feltWool',
      'crayonKids',
      'voxel',
      'origami',
      'glitchArt',
    ];
    for (const key of stylizedKeys) {
      const preset = VISUAL_STYLE_PRESETS.find((item) => item.key === key);
      expect(preset).toBeTruthy();
      const lower = preset!.prompt.toLowerCase();
      expect(
        /anime|ghibli|donghua|pixar|disney|illustration|watercolor|ink-wash|oil painting|comic|graphic novel|webtoon|manhwa|hand-drawn|painted|claymation|stop-motion|pixel|chibi|isometric|miniature|cel shading|3d cg animation|ukiyo-e|paper-cut|low-poly|cartoon|toon-shaded|unreal engine|lego|felted|crayon|charcoal|line-art|dunhuang|stained-glass|art nouveau|pop art|shadow puppet|gouache|shoujo|lianhuanhua|voxel|origami|glitch/.test(
          lower
        )
      ).toBe(true);
    }
  });

  test('live-action genre presets stay photoreal (no animation needles)', () => {
    for (const key of [
      'cinematic',
      'westernEpic',
      'horrorGothic',
      'sciFiClean',
      'editorial',
      'kDrama',
      'hkCinema',
      'idolMv',
      'xianxia',
      'a24Muted',
      'urbanRomance',
      'wuxia',
      'pastelTableau',
      'polaroid',
      'super8',
    ]) {
      const preset = VISUAL_STYLE_PRESETS.find((item) => item.key === key);
      expect(preset).toBeTruthy();
      const lower = preset!.prompt.toLowerCase();
      expect(lower.includes('anime')).toBe(false);
      expect(lower.includes('cartoon')).toBe(false);
      expect(lower.includes('pixar')).toBe(false);
      expect(lower.includes('ghibli')).toBe(false);
      expect(lower.includes('disney')).toBe(false);
      expect(lower.includes('comic')).toBe(false);
    }
  });

  test('catalog prompts stay brand-free (genre craft, not studio or director names)', () => {
    const brandNeedles = [
      'studio ghibli',
      'makoto shinkai',
      'pixar',
      'disney',
      'lego',
    ];
    for (const preset of VISUAL_STYLE_PRESETS) {
      const blob = `${preset.defaultLabel} ${preset.prompt}`.toLowerCase();
      for (const needle of brandNeedles) {
        expect(blob.includes(needle)).toBe(false);
      }
    }
  });
});
