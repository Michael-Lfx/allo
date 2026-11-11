import { describe, expect, test } from 'bun:test';
import { defaultDraft } from '../videoGeneration/home/homeDraft';
import type { VideoCreateDraft } from '../videoGeneration/home/types';
import {
  buildBriefingCreateInput,
  buildHomeVideoGenerateRequest,
  buildVideoSessionCreateInput,
  isDefinitelyNotCreatedCreateError,
  findUniqueClaimTask,
} from './homeVideoSubmit';
import { buildGenerateTaskPayload } from '../videoGeneration/home/submitDtos';

const draftWith = (patch: Partial<VideoCreateDraft>): VideoCreateDraft => ({
  ...defaultDraft(),
  ...patch,
});

describe('home video submit data flow', () => {
  test('generate DTO normalizes the model-selected duration and resolution', () => {
    const draft = draftWith({
      creationPrompt: '第一行标题\n第二行画面',
      style: '',
      preferences: {
        ...defaultDraft().preferences,
        models: {
          ...defaultDraft().preferences.models,
          video_model: 'minimax-h3',
        },
        resolution: '720p',
        targetDurationSecs: 99,
      },
    });
    const request = buildHomeVideoGenerateRequest(draft);
    expect(request.title).toBe('第一行标题');
    expect(request.prompt).toContain('第一行标题');
    expect(request.prompt).toContain('第二行画面');
    expect(request.model).toBe('minimax-h3');
    expect(request.resolution).toBe('768P');
    expect(request.durationSecs).toBeGreaterThanOrEqual(4);
    expect(request.durationSecs).toBeLessThanOrEqual(15);
  });

  test('agent session payload keeps workflow and derives action title from the character file', () => {
    const agent = buildVideoSessionCreateInput(draftWith({}));
    expect(agent).toEqual({ workflow: 'idea2video', title: undefined });

    const action = buildVideoSessionCreateInput(
      draftWith({
        workflow: 'action2video',
        actionCharacter: {
          file: new File([], 'hero-character.png'),
          previewUrl: '',
        },
      }),
    );
    expect(action).toEqual({ workflow: 'action2video', title: 'hero-character' });
  });

  test('briefing payload keeps source, depth and TTS/image model choices', () => {
    const draft = draftWith({
      sourceText: '今日新闻',
      sourceUrls: 'https://example.com/a, not-a-url',
      briefingFormatSecs: 60,
      researchDepth: 'deep',
      timeWindowHours: 12,
      briefingTts: { provider_id: 'tts-p', model: 'tts-m', voice: 'v1' },
      briefingImage: { provider_id: 'img-p', model: 'img-m' },
    });
    const request = buildBriefingCreateInput(draft);
    expect(request.intent).toBe('今日新闻');
    expect(request.title).toBe('今日新闻');
    expect(request.source_urls).toEqual(['https://example.com/a']);
    expect(request.format_secs).toBe(60);
    expect(request.research_depth).toBe('deep');
    expect(request.time_window_hours).toBe(12);
    expect(request.tts_provider_id).toBe('tts-p');
    expect(request.tts_model).toBe('tts-m');
    expect(request.tts_voice).toBe('v1');
    expect(request.image_provider_id).toBe('img-p');
    expect(request.image_model).toBe('img-m');
  });

  test('generate task API payload carries the exact input text and uploaded refs', () => {
    const draft = draftWith({
      creationPrompt: '  窗边的猫伸懒腰  ',
      style: '',
      preferences: {
        ...defaultDraft().preferences,
        models: {
          ...defaultDraft().preferences.models,
          video_model: 'minimax-h3',
        },
      },
    });
    const payload = buildGenerateTaskPayload(draft, ['media-1', 'media-2']);
    expect(payload.mode).toBe('video');
    expect(payload.prompt).toContain('窗边的猫伸懒腰');
    expect(payload.prompt).not.toContain('  窗边');
    expect(payload.model).toBe('minimax-h3');
    expect(payload.reference_media_ids).toEqual(['media-1', 'media-2']);
    expect(payload.duration_secs).toBeGreaterThanOrEqual(4);
    expect(payload.duration_secs).toBeLessThanOrEqual(15);
  });

  test('only deterministic 4xx errors are treated as "task definitely not created"', () => {
    const backendError = (status: number) =>
      Object.assign(new Error('backend'), {
        name: 'BackendHttpError',
        status,
        code: 'TEST',
      });
    expect(isDefinitelyNotCreatedCreateError(backendError(400))).toBe(true);
    expect(isDefinitelyNotCreatedCreateError(backendError(404))).toBe(true);
    expect(isDefinitelyNotCreatedCreateError(backendError(500))).toBe(false);
    expect(isDefinitelyNotCreatedCreateError(new Error('timeout'))).toBe(false);
  });

  test('unknown-outcome recovery only claims a unique reference-media set', () => {
    const tasks = [
      { task_id: 'one', reference_media_ids: ['m1', 'm2'] },
      { task_id: 'other', reference_media_ids: ['m2'] },
      { task_id: 'other-2', reference_media_ids: ['m2'] },
    ];
    expect(findUniqueClaimTask(tasks, ['m2', 'm1'])?.task_id).toBe('one');
    expect(findUniqueClaimTask(tasks, ['m2'])).toBeNull();
    expect(findUniqueClaimTask(tasks, [])).toBeNull();
  });
});
