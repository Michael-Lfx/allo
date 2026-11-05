import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { videoModelOptionMatches } from './videoModelTag';

const read = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');

describe('videoModelOptionMatches', () => {
  test('matches the catalog id and the display name', () => {
    expect(videoModelOptionMatches('h3', 'flowy/minimax-h3', 'MiniMax-H3')).toBe(true);
    expect(videoModelOptionMatches('Mini', 'flowy/minimax-h3', 'MiniMax-H3')).toBe(true);
    expect(videoModelOptionMatches('seedance', 'flowy/minimax-h3', 'MiniMax-H3')).toBe(false);
    expect(videoModelOptionMatches('  ', 'flowy/wan3.0-video', 'Wan 3.0')).toBe(true);
  });
});

describe('video model option labels', () => {
  test('keeps the name searchable and does not render marketing tags', () => {
    const zh = JSON.parse(read('../../services/i18n/locales/zh-CN/videoGeneration.json')) as {
      workspace: { models: Record<string, unknown> };
    };
    const en = JSON.parse(read('../../services/i18n/locales/en-US/videoGeneration.json')) as {
      workspace: { models: Record<string, unknown> };
    };
    expect(zh.workspace.models.tags).toBeUndefined();
    expect(en.workspace.models.tags).toBeUndefined();

    const label = read('./VideoModelOptionLabel.tsx');
    const selectors = read('../../pages/videoGeneration/components/ModelSelectors.tsx');
    const popover = read('../../pages/videoGeneration/home/GenerationPreferencesPopover.tsx');
    const canvasPicker = read('../../pages/videoCanvas/oc/components/model-picker.tsx');
    expect(label.includes('性价比')).toBe(false);
    expect(label.includes('高通过率')).toBe(false);
    for (const source of [selectors, popover, canvasPicker]) {
      expect(source.includes('VideoModelOptionLabel')).toBe(true);
      expect(source.includes('chat-model-plan-badge')).toBe(true);
      expect(source.includes('性价比')).toBe(false);
      expect(source.includes('高通过率')).toBe(false);
    }
    expect(selectors.includes('videoModelSelectFilter')).toBe(true);
    expect(popover.includes('videoModelSelectClassName')).toBe(true);
  });
});
