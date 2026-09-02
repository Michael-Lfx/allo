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
});
