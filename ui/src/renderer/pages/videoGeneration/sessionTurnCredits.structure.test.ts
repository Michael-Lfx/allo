import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const source = (rel: string) =>
  readFileSync(new URL(rel, import.meta.url), 'utf8');

describe('video generation session video credits', () => {
  test('workspace and progress rail surface persisted video-task credits', () => {
    const page = source('./WorkspacePage.tsx');
    const timeline = source('./components/ProgressTimeline.tsx');
    expect(page.includes('credits_consumed')).toBe(true);
    expect(page.includes('session-video-credits')).toBe(true);
    expect(timeline.includes('creditsConsumed')).toBe(true);
    expect(timeline.includes('session-video-credits-live')).toBe(true);
  });

  test('artifact preview no longer offers local image replace', () => {
    const preview = source('./components/ArtifactPreviewPanel.tsx');
    expect(preview.includes('replaceImage')).toBe(false);
    expect(preview.includes('replaceArtifactFile')).toBe(false);
    expect(preview.includes('本地替换')).toBe(false);
  });
});
