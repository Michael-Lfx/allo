import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import type { TFunction } from 'i18next';
import { describeClipFailure, formatClipOperationError } from './clipFailure';

const t = ((key: string, opts?: { defaultValue?: string }) =>
  opts?.defaultValue ?? key) as TFunction;

const locale = (lang: 'zh-CN' | 'en-US') =>
  JSON.parse(
    readFileSync(
      new URL(`../../services/i18n/locales/${lang}/videoGeneration.json`, import.meta.url),
      'utf8'
    )
  ) as {
    clip: {
      failure: { creditsTitle: string; creditsHint: string; creditsToast: string };
    };
  };

describe('describeClipFailure', () => {
  test('maps API 402 / insufficient credits to a billing recovery', () => {
    const result = describeClipFailure('Internal error: API error 402: 积分不足', t);
    expect(result.credits).toBe(true);
    expect(result.title).toBe('积分不足');
    expect(result.message).toContain('购买积分');
    expect(result.message).not.toContain('从断点继续');
    expect(result.recovery).toEqual({
      labelKey: 'billing.openBilling',
      source: 'open_billing',
    });
  });

  test('maps the vimax INSUFFICIENT_CREDITS marker', () => {
    const result = describeClipFailure(
      'Video generation failed\nHint: INSUFFICIENT_CREDITS — Flowy credits are too low.',
      t
    );
    expect(result.credits).toBe(true);
    expect(result.recovery?.source).toBe('open_billing');
  });

  test('keeps non-credit failures off the billing path', () => {
    const result = describeClipFailure(
      'Bad request: 首帧/尾帧必须是图片（PNG/JPEG/WebP），不能使用音频或视频。',
      t
    );
    expect(result.credits).toBe(false);
    expect(result.recovery).toBeNull();
    expect(result.message).toContain('图片');
    expect(result.message).not.toContain('Bad request');
  });

  test('falls back when the backend stored an empty error', () => {
    const result = describeClipFailure('', t);
    expect(result.credits).toBe(false);
    expect(result.message).toBe('视频生成失败');
  });
});

describe('formatClipOperationError', () => {
  test('uses the credits toast instead of prefixing a 402 dump', () => {
    const result = formatClipOperationError(
      new Error('API error 402: Insufficient Balance'),
      t,
      { key: 'videoGeneration.create.generateFailed', defaultValue: '视频生成创建失败' }
    );
    expect(result.credits).toBe(true);
    expect(result.message).toBe('积分不足，请充值后再试。');
    expect(result.message).not.toContain('402');
    expect(result.message).not.toContain('Insufficient Balance');
  });

  test('prefixes localized generation errors for other failures', () => {
    const result = formatClipOperationError(
      new Error('Bad request: 首帧/尾帧必须是图片（PNG/JPEG/WebP），不能使用音频或视频。'),
      t,
      { key: 'videoGeneration.create.generateFailed', defaultValue: '视频生成创建失败' }
    );
    expect(result.credits).toBe(false);
    expect(result.message.startsWith('视频生成创建失败: ')).toBe(true);
    expect(result.message).toContain('图片');
    expect(result.message).not.toContain('Bad request');
  });
});

describe('clip failure follows UI language', () => {
  test('credits copy comes from the active t() locale, not the backend dump', async () => {
    const { createInstance } = await import('i18next');
    const en = locale('en-US');
    const i18n = createInstance();
    await i18n.init({
      lng: 'en-US',
      fallbackLng: 'en-US',
      resources: { 'en-US': { translation: { videoGeneration: en } } },
      interpolation: { escapeValue: false },
    });
    const result = describeClipFailure('Internal error: API error 402: 积分不足', i18n.t.bind(i18n));
    expect(result.credits).toBe(true);
    expect(result.title).toBe('Insufficient credits');
    expect(result.message).toContain('Buy credits');
    expect(result.message).not.toContain('积分不足');
    expect(result.message).not.toContain('API error 402');
  });
});

describe('clip failure locale dictionaries', () => {
  test('zh-CN and en-US both describe top-up without workshop resume copy', () => {
    const zh = locale('zh-CN').clip.failure;
    const en = locale('en-US').clip.failure;
    expect(zh.creditsTitle).toBe('积分不足');
    expect(en.creditsTitle).toBe('Insufficient credits');
    expect(zh.creditsHint).toContain('购买积分');
    expect(en.creditsHint.toLowerCase()).toContain('buy credits');
    expect(zh.creditsHint).not.toContain('从断点继续');
    expect(en.creditsHint.toLowerCase()).not.toContain('checkpoint');
    expect(zh.creditsToast).toContain('充值');
    expect(en.creditsToast.toLowerCase()).toContain('top up');
  });
});
