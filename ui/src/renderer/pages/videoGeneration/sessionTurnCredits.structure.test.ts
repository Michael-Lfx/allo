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
    const board = source('./components/StoryboardBoard.tsx');
    expect(board.includes('shot-video-credits')).toBe(true);
    expect(board.includes('creditsForScene')).toBe(true);
    expect(board.includes('packedBeatsHint')).toBe(false);
  });

  test('agent session messages expose a custom vertical scroll rail', () => {
    const session = source('./studioAgentSession/StudioAgentSession.tsx');
    const css = source('./studioAgentSession/index.module.css');
    expect(session.includes('studio-session-scroll-rail')).toBe(true);
    expect(session.includes('StudioSessionScrollRail')).toBe(true);
    expect(css.includes('.scrollRail {')).toBe(true);
    expect(css.includes('.scrollThumb {')).toBe(true);
  });

  test('storyboard shot detail is read-only', () => {
    const modal = source('./components/StoryboardShotEditorModal.tsx');
    expect(modal.includes('saveShotCopy')).toBe(false);
    expect(modal.includes('TextArea')).toBe(false);
    expect(modal.includes("t('common.close'")).toBe(true);
    expect(modal.includes('onSaved')).toBe(false);
  });

  test('technical artifact tree is gated on developer mode', () => {
    const page = source('./WorkspacePage.tsx');
    expect(page.includes('useDeveloperModeGate')).toBe(true);
    expect(page.includes('{developerMode && artifacts.length > 0 ? (')).toBe(true);
    expect(page.includes("t('videoGeneration.studio.technicalDetails'")).toBe(true);
  });

  test('artifact preview no longer offers local image replace', () => {
    const preview = source('./components/ArtifactPreviewPanel.tsx');
    expect(preview.includes('replaceImage')).toBe(false);
    expect(preview.includes('replaceArtifactFile')).toBe(false);
    expect(preview.includes('本地替换')).toBe(false);
  });
});
