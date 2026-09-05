import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, test } from 'bun:test';

import { lookbookCanvasStylePresets } from '@oc/lib/canvas/canvas-style-system';
import { VISUAL_STYLE_PRESETS } from '../visualStylePresets';
import { lookCoverFileId } from './lookCovers';
import {
  CANVAS_LOOKS,
  FEATURED_LOOK_IDS,
  HOME_LOOKS,
  composeClipPrompt,
  featuredLooks,
  lookById,
  lookByPrompt,
  lookToCanvasPreset,
} from './looks';

describe('unified look catalog', () => {
  test('home looks include every vimax preset plus unmapped lookbook extras', () => {
    const ids = HOME_LOOKS.map((look) => look.id);
    expect(new Set(ids).size).toBe(ids.length);
    expect(HOME_LOOKS.length).toBeGreaterThanOrEqual(VISUAL_STYLE_PRESETS.length);
    for (const preset of VISUAL_STYLE_PRESETS) {
      expect(lookById.get(preset.key)?.modelPrompt).toBe(preset.prompt);
      expect(lookByPrompt.get(preset.prompt)?.id).toBe(preset.key);
    }
    expect(lookById.has('court-pageant')).toBe(true);
    expect(lookById.has('nature-healing')).toBe(true);
    expect(lookById.has('space-opera')).toBe(true);
  });

  test('featured looks are the four former creation skills', () => {
    expect(featuredLooks().map((look) => look.id)).toEqual([...FEATURED_LOOK_IDS]);
    expect(FEATURED_LOOK_IDS).toEqual(['cinematic', 'anime', 'cyberpunk', 'inkWash']);
  });

  test('canvas recommended looks stay the lookbook subset', () => {
    expect(CANVAS_LOOKS.map((look) => look.canvasPresetId)).toEqual(
      lookbookCanvasStylePresets.map((preset) => preset.id),
    );
    expect(CANVAS_LOOKS.every((look) => look.cover.from && look.cover.to)).toBe(true);
  });

  test('every catalog look has an original webp still in public/looks', () => {
    const looksDir = join(process.cwd(), 'public', 'looks');
    const fileIds = new Set(
      [...HOME_LOOKS, ...CANVAS_LOOKS].map((look) => lookCoverFileId(look.id)),
    );
    expect(fileIds.size).toBeGreaterThanOrEqual(VISUAL_STYLE_PRESETS.length);
    for (const fileId of fileIds) {
      expect(fileId.includes('/')).toBe(false);
      expect(existsSync(join(looksDir, `${fileId}.webp`))).toBe(true);
    }
    for (const look of [...HOME_LOOKS, ...CANVAS_LOOKS]) {
      expect(look.cover.image).toMatch(/looks\/[\w-]+\.webp$/);
      expect(look.cover.image).not.toContain('short-drama-styles');
      expect(lookToCanvasPreset(look).cover.image).toBe(look.cover.image);
    }
  });

  test('clip prompt prepends look without duplicating it', () => {
    const look = 'cinematic film look';
    expect(composeClipPrompt('a cat runs', look)).toBe(`${look}\n\na cat runs`);
    expect(composeClipPrompt(`${look}\n\na cat runs`, look)).toBe(`${look}\n\na cat runs`);
    expect(composeClipPrompt('a cat runs', '')).toBe('a cat runs');
    expect(composeClipPrompt('', look)).toBe(look);
  });
});
