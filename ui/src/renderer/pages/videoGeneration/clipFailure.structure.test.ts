import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const source = (rel: string) => readFileSync(new URL(rel, import.meta.url), 'utf8');

describe('clip result failure surface', () => {
  test('localizes stored backend errors at render time', () => {
    const page = source('./ClipResultPage.tsx');
    expect(page.includes('describeClipFailure(rawError, t)')).toBe(true);
    expect(page.includes("setError(result.error")).toBe(false);
    expect(page.includes('formatClipOperationError')).toBe(true);
  });

  test('credits failures open the official website credits tab', () => {
    const page = source('./ClipResultPage.tsx');
    const css = source('./ClipResultPage.module.css');
    expect(page.includes("data-testid='video-failure-open-billing'")).toBe(true);
    expect(page.includes('openOfficialWebsiteCredits')).toBe(true);
    expect(page.includes("source: 'video_failure_card'")).toBe(true);
    expect(page.includes("feature: 'video_generation'")).toBe(true);
    expect(page.includes("t('billing.openBilling'")).toBe(true);
    expect(css.includes('.errorPanel {')).toBe(true);
    expect(css.includes('.errorPanelActions {')).toBe(true);
  });
});

describe('clip home submit credits toast', () => {
  test('create-generate maps 402 through the shared clip error helper', () => {
    const page = source('./index.tsx');
    const generateHandler = page.slice(
      page.indexOf('const handleCreateGenerate'),
      page.indexOf('const handleCreateBriefing')
    );
    expect(generateHandler.includes('formatClipOperationError')).toBe(true);
    expect(generateHandler.includes("source: 'video_launch'")).toBe(true);
    expect(generateHandler.includes('trackLowCreditBalance')).toBe(true);
    expect(generateHandler.includes('cause.message : String(cause)')).toBe(false);
  });
});
