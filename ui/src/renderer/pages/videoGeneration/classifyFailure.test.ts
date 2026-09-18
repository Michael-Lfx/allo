import { describe, expect, test } from 'bun:test';
import type { TFunction } from 'i18next';
import { classifyFailure } from './classifyFailure';

const t = ((key: string, opts?: { defaultValue?: string }) =>
  opts?.defaultValue ?? key) as TFunction;

describe('classifyFailure', () => {
  test('plan_scene empty JSON is not an LLM failure', () => {
    const result = classifyFailure(
      'JSON error: EOF while parsing a value at line 1 column 0',
      'plan_scene',
      [
        {
          stage: 'plan_scene',
          message: '正在规划 0/7 个场景文本产物',
          at: 'a',
        },
      ],
      t
    );
    expect(result.kind).toBe('unknown');
    expect(result.title).toBe('规划产物读写失败');
  });

  test('true planning LLM failures stay llm', () => {
    const result = classifyFailure(
      'LLM failed: empty chat completion (model returned no content)',
      'plan_scene',
      [{ stage: 'plan_scene', message: '', at: 'a' }],
      t
    );
    expect(result.kind).toBe('llm');
  });

  test('seedance copyright policy is a user-facing moderation failure', () => {
    const result = classifyFailure(
      'video generation failed: Scene 2/7 render failed: Shot 1: video generation failed: OutputVideoSensitiveContentDetected.PolicyViolation: The request failed because the output video may be related to copyright restrictions.',
      'video_poll',
      [{ stage: 'video_poll', message: '', at: 'a' }],
      t
    );
    expect(result.kind).toBe('moderation');
    expect(result.title).toBe('成片未通过版权审核');
    expect(result.errorCode).toBe('OutputVideoSensitiveContentDetected.PolicyViolation');
    expect(result.providerMessage).toContain('copyright restrictions');
  });

  test('scene render video failure is not classified as LLM', () => {
    const result = classifyFailure(
      'video generation failed: Shot 0: OutputVideoSensitiveContentDetected.PolicyViolation: copyright restrictions.',
      'render_scene',
      [
        {
          stage: 'render_scene',
          message: '正在渲染场景（1/5）· 含图片与视频模型',
          at: 'a',
        },
      ],
      t
    );
    expect(result.kind).toBe('moderation');
  });

  test('render_scene plus video generation failed is video not llm', () => {
    const result = classifyFailure(
      'video generation failed: Shot 0: Model call failed. Please try again later',
      'render_scene',
      [
        {
          stage: 'render_scene',
          message: '正在渲染场景（1/5）· 含图片与视频模型',
          at: 'a',
        },
      ],
      t
    );
    expect(result.kind).toBe('video');
  });

  test('legacy render_scene_failed wrap still classifies as video', () => {
    const result = classifyFailure(
      'Failed at stage `render_scene_failed`\nPrevious status: Scene 1/5 failed; 0 scene(s) already on disk — resume from checkpoint\n\nvideo generation failed: Scene 1/5 render failed: Shot 0: model call failed',
      'render_scene_failed',
      [
        {
          stage: 'render_scene_failed',
          message: 'Scene 1/5 failed; 0 scene(s) already on disk — resume from checkpoint',
          at: 'a',
        },
      ],
      t
    );
    expect(result.kind).toBe('video');
    expect(result.title).toBe('videoGeneration.workspace.failure.videoTitle');
  });

  test('empty-set plate during world assets is an image failure', () => {
    const result = classifyFailure(
      'Failed at stage `world_assets_start`\nPrevious status: 世界参考图生成失败\n\nimage generation failed: empty-set plate still contains people after retries: C:\\film\\env.png',
      'world_assets_start',
      [{ stage: 'world_assets_start', message: '世界参考图生成失败', at: 'a' }],
      t
    );
    expect(result.kind).toBe('image');
  });

  test('wan3 reference_audio duration cap is a dedicated video failure', () => {
    const result = classifyFailure(
      'video generation failed: InvalidParameter: reference_audio total duration 15.6s exceeds max 15s',
      'video_poll',
      [{ stage: 'video_poll', message: '', at: 'a' }],
      t
    );
    expect(result.kind).toBe('video');
    expect(result.title).toBe('参考音频总时长超限');
    expect(result.hint).toContain('15');
  });

  test('seedance per-clip audio floor is not the wan 15s cap', () => {
    const result = classifyFailure(
      'The parameter `content[4]` specified in the request is not valid: the parameter audio duration (seconds) specified in the request must be greater than or equal to 1.8 for model doubao-seedance-2-0-fast in r2v. Request id: 021789715359358873b68ad3a5b1c0a1311de00f33c085717a7d2',
      'video_poll',
      [{ stage: 'video_poll', message: '', at: 'a' }],
      t
    );
    expect(result.kind).toBe('video');
    expect(result.title).toBe('参考音频单段过短');
    expect(result.hint).toContain('1.8');
    expect(result.hint).not.toContain('合计不超过');
  });
});
