import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const source = (rel: string) => readFileSync(new URL(rel, import.meta.url), 'utf8');

describe('clip result page shell and retry', () => {
  test('scrolls inside the app shell and does not treat millis as seconds', () => {
    const page = source('./ClipResultPage.tsx');
    const css = source('./ClipResultPage.module.css');
    expect(page.includes('flex-1 min-h-0')).toBe(true);
    expect(page.includes('overflow-y-auto')).toBe(true);
    expect(page.includes('clipTimestampMs(')).toBe(true);
    expect(page.includes('created_at * 1000')).toBe(false);
    expect(page.includes('min-height: 100vh')).toBe(false);
    expect(css.includes('100vh')).toBe(false);
    expect(css.includes('width: min(')).toBe(true);
  });

  test('a failed clip starts a new task with the original audio and reference video', () => {
    const page = source('./ClipResultPage.tsx');
    const model = source('./clipResultModel.ts');
    expect(page.includes('clipRegenerateBody(')).toBe(true);
    expect(page.includes('clipFollowUp(')).toBe(true);
    expect(page.includes('video.paused')).toBe(true);
    expect(model.includes('audio_media_ids')).toBe(true);
    expect(model.includes('reference_video_media_id')).toBe(true);
  });
});
