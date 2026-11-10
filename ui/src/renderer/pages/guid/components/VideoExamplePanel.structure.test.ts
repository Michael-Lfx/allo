import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const read = (relative: string) => readFileSync(new URL(relative, import.meta.url), 'utf8');
const guide = read('../GuidPage.tsx');
const panel = read('./VideoExamplePanel.tsx');
const composer = read('../../videoGeneration/home/VideoHomeComposer.tsx');
const submitHook = read('../homeVideoSubmit.ts');

describe('homepage video entry contract', () => {
  test('opens a preview-only drawer instead of replacing the prompt', () => {
    expect(guide).toContain('VideoExamplePanel');
    expect(guide).toContain('videoPanelOpen');
    expect(guide).toContain('closeVideoPanel');
    expect(guide).not.toContain('handleSelectVideoExample');
    expect(guide).not.toContain('setHomeVideoText(video.title');
    expect(panel).toContain('listTvShow');
    expect(panel).toContain('guid-home-video-panel');
    expect(panel).toContain('guid-home-video-preview');
    expect(panel).toContain('previewVideo');
    expect(panel).not.toContain('onSelect');
    expect(panel).not.toContain('做这支视频');
  });

  test('reuses the original video home composer and its submit actions', () => {
    expect(guide).toContain('VideoHomeComposer');
    expect(guide).toContain('useHomeVideoSubmit');
    expect(guide).not.toContain('HomeVideoComposer');
    expect(guide).toContain('showHero={false}');
    expect(guide).toContain('showCampaign={false}');
    expect(composer).toContain('ModeMenu');
    expect(composer).toContain('GenerationPreferencesPopover');
    expect(composer).toContain('SlashSkillMenu');
    expect(composer).toContain('VerticalSkillMenu');
    expect(composer).toContain('LookPicker');
    expect(submitHook).toContain('handleCreate');
    expect(submitHook).toContain('handleCreateCanvas');
    expect(submitHook).toContain('handleCreateGenerate');
    expect(submitHook).toContain('handleCreateBriefing');
    expect(submitHook).toContain('/video-generation/clip/');
    expect(submitHook).toContain('briefingWorkspacePath');
    expect(submitHook).toContain('videoCanvasProjectPath');
    expect(submitHook).toContain('rememberVideoGenerationTask');
    expect(submitHook).toContain('postIssued');
    expect(submitHook).toContain('generateRecovery');
    expect(guide).toContain('guid-home-video-generate-recovery');
    expect(guide).toContain('resolveGenerateRecovery');
    expect(guide).not.toContain('onExit={backFromHomePanel}');
  });
});
