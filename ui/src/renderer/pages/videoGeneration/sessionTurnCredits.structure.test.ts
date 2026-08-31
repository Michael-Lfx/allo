import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const source = (rel: string) =>
  readFileSync(new URL(rel, import.meta.url), 'utf8');

describe('video generation session video credits', () => {
  test('workspace and progress rail surface persisted video-task credits', () => {
    const page = source('./WorkspacePage.tsx');
    const session = source('./studioAgentSession/StudioAgentSession.tsx');
    expect(page.includes('credits_consumed')).toBe(true);
    expect(page.includes('session-video-credits')).toBe(true);
    expect(session.includes('creditsConsumed')).toBe(true);
    expect(session.includes('session-video-credits-live')).toBe(true);
    const credits = source('./sessionCredits.ts');
    expect(credits.includes('video_credits')).toBe(true);
  });

  test('artifact preview no longer offers local image replace', () => {
    const preview = source('./components/ArtifactPreviewPanel.tsx');
    expect(preview.includes('replaceImage')).toBe(false);
    expect(preview.includes('replaceArtifactFile')).toBe(false);
    expect(preview.includes('本地替换')).toBe(false);
  });
});
