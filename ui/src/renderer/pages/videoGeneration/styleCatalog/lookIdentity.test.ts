import { describe, expect, test } from 'bun:test';

import { resolveCanvasStylePreset } from '@oc/lib/canvas/canvas-style-system';
import { promptForVisualStyleKey } from '../visualStylePresets';
import {
  CREATION_SKILL_CANVAS_PRESET,
  canvasPresetIdForCreationSkill,
  resolveLookIdentity,
} from './lookIdentity';

describe('look identity', () => {
  test('creation skills resolve to real canvas lookbook presets', () => {
    for (const [skillId, canvasId] of Object.entries(CREATION_SKILL_CANVAS_PRESET)) {
      const identity = resolveLookIdentity({ creationSkillId: skillId });
      expect(identity?.canvasPresetId).toBe(canvasId);
      expect(resolveCanvasStylePreset(canvasId)?.id).toBe(canvasId);
      expect(identity?.canvasPrompt).toContain('【题材世界观】');
      expect(canvasPresetIdForCreationSkill(skillId as keyof typeof CREATION_SKILL_CANVAS_PRESET)).toBe(
        canvasId,
      );
    }
  });

  test('cinematic creation skill is urban live-action, not an unknown skill id', () => {
    const identity = resolveLookIdentity({ creationSkillId: 'cinematic' });
    expect(identity?.canvasPresetId).toBe('urban-live-action');
    expect(resolveCanvasStylePreset('cinematic')).toBeUndefined();
    expect(identity?.vimaxKey).toBe('cinematic');
    expect(identity?.modelPrompt).toBe(promptForVisualStyleKey('cinematic'));
  });

  test('home prompt text round-trips through vimax catalog into canvas spec', () => {
    const prompt = promptForVisualStyleKey('inkWash');
    const identity = resolveLookIdentity({ stylePrompt: prompt });
    expect(identity?.vimaxKey).toBe('inkWash');
    expect(identity?.canvasPresetId).toBe('ink-narrative');
    expect(identity?.canvasTitle).toBe('水墨叙事');
  });

  test('lookbook slug and v2 combo ids stay canvas-native', () => {
    const lookbook = resolveLookIdentity({ canvasPresetId: 'cyberpunk-neon' });
    expect(lookbook?.canvasPresetId).toBe('cyberpunk-neon');
    expect(lookbook?.selection).toEqual({
      world: 'cyberpunk',
      tone: 'dark',
      medium: 'live-action',
      character: 'realistic',
    });

    const comboId = 'v2-wasteland--dark--live-action--realistic';
    const combo = resolveLookIdentity({ canvasPresetId: comboId });
    expect(combo?.canvasPresetId).toBe(comboId);
    expect(combo?.selection?.world).toBe('wasteland');
  });

  test('unmapped vimax looks keep a look: alias instead of a fake lookbook slug', () => {
    const identity = resolveLookIdentity({ vimaxKey: 'pixelArt' });
    expect(identity?.vimaxKey).toBe('pixelArt');
    expect(identity?.canvasPresetId).toBe('look:pixelArt');
    expect(identity?.canvasPrompt).toBe(identity?.modelPrompt);
    expect(resolveCanvasStylePreset('look:pixelArt')).toBeUndefined();
  });

  test('style prompt wins over a stale creation skill id', () => {
    const identity = resolveLookIdentity({
      creationSkillId: 'cinematic',
      stylePrompt: promptForVisualStyleKey('pixelArt'),
    });
    expect(identity?.vimaxKey).toBe('pixelArt');
    expect(identity?.canvasPresetId).toBe('look:pixelArt');
  });

  test('unknown input is null, empty prompt is not cinematic', () => {
    expect(resolveLookIdentity({})).toBeNull();
    expect(resolveLookIdentity({ stylePrompt: '' })).toBeNull();
    expect(resolveLookIdentity({ stylePrompt: 'a bespoke neon alley' })).toBeNull();
  });
});
