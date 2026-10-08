import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { parseSessionRenderMode, resolveSessionRenderMode } from './types';

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
    expect(board.includes('shotCardCredits')).toBe(true);
    expect(board.includes('shotCreditsBadge')).toBe(false);
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

  test('storyboard shot inspector is an editable director desk', () => {
    const inspector = source('./components/ShotPacketInspector.tsx');
    const board = source('./components/StoryboardBoard.tsx');
    const modal = source('./components/StoryboardShotEditorModal.tsx');
    expect(inspector.includes('putShotPacket')).toBe(true);
    expect(inspector.includes('planning = false')).toBe(true);
    expect(inspector.includes('cannot edit a shot packet while planning')).toBe(true);
    expect(board.includes('planning={planning}')).toBe(true);
    expect(inspector.includes('compiledPrompt') || inspector.includes('compiled_prompt')).toBe(true);
    expect(inspector.includes('takes')).toBe(true);
    expect(inspector.includes('shot-graph')).toBe(true);
    const infinite = source('./shotInfiniteCanvas/ShotInfiniteCanvas.tsx');
    expect(inspector.includes('shot-api-prompt') || infinite.includes('shot-api-prompt')).toBe(true);
    expect(inspector.includes('ShotInfiniteCanvas')).toBe(true);
    expect(inspector.includes('PanZoomViewport')).toBe(false);
    expect(infinite.includes('InfiniteCanvas')).toBe(true);
    expect(infinite.includes('CanvasNodePromptPanel')).toBe(true);
    expect(infinite.includes('CanvasNodeInfoModal')).toBe(true);
    expect(inspector.includes('VIDEO_NODE_ID')).toBe(true);
    expect(inspector.includes("defaultValue: '重渲'")).toBe(false);
    expect(inspector.includes("defaultValue: '恢复编译'")).toBe(false);
    expect(infinite.includes('shotCanvasStage')).toBe(true);
    expect(infinite.includes('shot-canvas-fullscreen')).toBe(true);
    expect(infinite.includes('shotBoundMentionReferences')).toBe(true);
    expect(inspector.includes('nodeMentionsToSeedancePrompt')).toBe(true);
    expect(inspector.includes('imageModel')).toBe(true);
    expect(board.includes('imageModel')).toBe(true);
    expect(board.includes('videoModel')).toBe(true);
    expect(inspector.includes('visualDirection')).toBe(false);
    expect(inspector.includes('audioDirection')).toBe(false);
    expect(board.includes('shot-review-bar')).toBe(true);
    expect(board.includes('approveShot')).toBe(true);
    expect(board.includes('storyboardFilmstripBadge')).toBe(true);
    expect(board.includes('shotThumbMetaBr')).toBe(true);
    expect(inspector.includes('first_frame')).toBe(false);
    expect(inspector.includes('content[]')).toBe(false);
    expect(modal.includes('TextArea')).toBe(false);
    expect(modal.includes('onSaved')).toBe(false);
  });

  test('storyboard fills the studio column and agent session can collapse to a rail', () => {
    const page = source('./WorkspacePage.tsx');
    const css = source('./index.module.css');
    const session = source('./studioAgentSession/StudioAgentSession.tsx');
    expect(page.includes('studioMainInnerWide')).toBe(true);
    expect(page.includes('data-studio-main')).toBe(true);
    expect(page.includes('sessionCollapsedRail')).toBe(true);
    expect(css.includes('.shotGraph {')).toBe(true);
    expect(css.includes('.shotInfiniteCanvas {')).toBe(true);
    expect(css.includes('.shotCanvasStage {')).toBe(true);
    expect(css.includes('.shotThumbMetaBr {')).toBe(true);
    expect(css.includes('.shotCanvasFullBtn {')).toBe(true);
    expect(css.includes('.studioMainInnerWide {')).toBe(true);
    expect(session.includes('onCollapsedChange(true)')).toBe(true);
    expect(session.includes('collapseText')).toBe(true);
  });

  test('film render mode is a session-level control on the studio header', () => {
    const page = source('./WorkspacePage.tsx');
    const feed = source('./useRunStatusFeed.ts');
    expect(page.includes("data-testid='session-render-mode'")).toBe(true);
    expect(page.includes('renderModeCluster')).toBe(true);
    expect(page.includes('setSessionRenderMode')).toBe(true);
    expect(page.includes("if (!isActiveStatus(statusFlags.status)) return")).toBe(false);
    const headerIdx = page.indexOf("data-testid='session-render-mode'");
    const storyboardIdx = page.indexOf("videoGeneration.studio.storyboard.title");
    expect(headerIdx).toBeGreaterThan(-1);
    expect(storyboardIdx).toBeGreaterThan(headerIdx);
    expect(feed.includes("renderMode: parseSessionRenderMode(next?.render_mode)")).toBe(true);
    const board = source('./components/StoryboardBoard.tsx');
    expect(board.includes("patchRunStatus({ render_mode: 'continuous' })")).toBe(true);
    expect(board.includes('session-render-mode')).toBe(false);
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

describe('session render mode helpers', () => {
  test('accepts persisted continuous and shot_review values', () => {
    expect(parseSessionRenderMode('shot_review')).toBe('shot_review');
    expect(parseSessionRenderMode('continuous')).toBe('continuous');
    expect(parseSessionRenderMode('')).toBeNull();
    expect(resolveSessionRenderMode(undefined)).toBe('continuous');
    expect(resolveSessionRenderMode('shot_review')).toBe('shot_review');
  });
});
