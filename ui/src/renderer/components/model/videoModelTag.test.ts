import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { videoModelOptionMatches, videoModelTagKind } from './videoModelTag';

const read = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');

describe('videoModelTagKind', () => {
  test('marks MiniMax-H3 as value and Wan 3.0 as high pass rate', () => {
    expect(videoModelTagKind('flowy/minimax-h3')).toBe('value');
    expect(videoModelTagKind('AIPC-MiniMax-H3-video')).toBe('value');
    expect(videoModelTagKind('minimax_h3')).toBe('value');
    expect(videoModelTagKind('flowy/wan3.0-video')).toBe('passRate');
    expect(videoModelTagKind('AIPC-wan3.0-video-prime')).toBe('passRate');
    expect(videoModelTagKind('Wan 3.0')).toBe('passRate');
    expect(videoModelTagKind('flowy::wan3.0-video')).toBe('passRate');
  });

  test('leaves other video models untagged', () => {
    expect(videoModelTagKind('')).toBeNull();
    expect(videoModelTagKind('flowy/doubao-seedance-2-0-fast')).toBeNull();
    expect(videoModelTagKind('minimax-hailuo-2.3')).toBeNull();
    expect(videoModelTagKind('wan2.1-video')).toBeNull();
  });
});

describe('videoModelOptionMatches', () => {
  test('matches the catalog id and the display name', () => {
    expect(videoModelOptionMatches('h3', 'flowy/minimax-h3', 'MiniMax-H3')).toBe(true);
    expect(videoModelOptionMatches('Mini', 'flowy/minimax-h3', 'MiniMax-H3')).toBe(true);
    expect(videoModelOptionMatches('seedance', 'flowy/minimax-h3', 'MiniMax-H3')).toBe(false);
    expect(videoModelOptionMatches('  ', 'flowy/wan3.0-video', 'Wan 3.0')).toBe(true);
  });
});

describe('video model tag copy', () => {
  test('declares both locale strings and wires them into the video model pickers', () => {
    const zh = JSON.parse(
      read('../../services/i18n/locales/zh-CN/videoGeneration.json')
    ) as {
      workspace: { models: { tags: { value: string; passRate: string } } };
    };
    const en = JSON.parse(
      read('../../services/i18n/locales/en-US/videoGeneration.json')
    ) as {
      workspace: { models: { tags: { value: string; passRate: string } } };
    };
    expect(zh.workspace.models.tags).toEqual({ value: '性价比', passRate: '高通过率' });
    expect(en.workspace.models.tags.value).toBe('Best value');
    expect(en.workspace.models.tags.passRate).toBe('High pass rate');

    const selectors = read('../../pages/videoGeneration/components/ModelSelectors.tsx');
    const popover = read('../../pages/videoGeneration/home/GenerationPreferencesPopover.tsx');
    const canvasPicker = read('../../pages/videoCanvas/oc/components/model-picker.tsx');
    for (const source of [selectors, popover, canvasPicker]) {
      expect(source.includes('VideoModelOptionLabel')).toBe(true);
    }
    expect(selectors.includes('videoModelSelectFilter')).toBe(true);
    expect(popover.includes('videoModelSelectClassName')).toBe(true);
  });
});
