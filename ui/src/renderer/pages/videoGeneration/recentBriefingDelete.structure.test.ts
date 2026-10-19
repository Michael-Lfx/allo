import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const source = (rel: string) => readFileSync(new URL(rel, import.meta.url), 'utf8');

const locale = (lang: 'zh-CN' | 'en-US') =>
  JSON.parse(
    readFileSync(
      new URL(`../../services/i18n/locales/${lang}/videoGeneration.json`, import.meta.url),
      'utf8'
    )
  ) as {
    list: { briefingKind: string };
    clip: { fullscreen: string; exitFullscreen: string };
  };

describe('recent briefing delete', () => {
  test('home list wires briefing cards to the delete API', () => {
    const page = source('./index.tsx');
    const api = source('./briefing/api.ts');
    const card = source('./components/BriefingCard.tsx');
    expect(api.includes('export async function deleteBriefing')).toBe(true);
    expect(api.includes("'DELETE'")).toBe(true);
    expect(page.includes('deleteBriefing')).toBe(true);
    expect(page.includes('onDelete={onDeleteBriefing}')).toBe(true);
    expect(page.includes('BriefingCard')).toBe(true);
    expect(card.includes("data-testid='briefing-card-delete'")).toBe(true);
    expect(card.includes('onDelete')).toBe(true);
  });

  test('zh-CN and en-US declare briefing kind labels', () => {
    const zh = locale('zh-CN');
    const en = locale('en-US');
    expect(zh.list.briefingKind).toBe('资讯口播');
    expect(en.list.briefingKind).toBe('Spoken briefing');
  });
});

describe('clip result maximize', () => {
  test('fullscreen targets the video player, not the page shell', () => {
    const page = source('./ClipResultPage.tsx');
    const css = source('./ClipResultPage.module.css');
    expect(page.includes('requestElementFullscreen(player)')).toBe(true);
    expect(page.includes("data-testid='clip-player-fullscreen'")).toBe(true);
    expect(page.includes('containerRef.current.requestFullscreen')).toBe(false);
    expect(page.includes('videoCssFullscreen')).toBe(true);
    expect(css.includes('.videoContainer:fullscreen')).toBe(true);
    expect(css.includes('.videoCssFullscreen')).toBe(true);
  });

  test('zh-CN and en-US declare maximize labels', () => {
    const zh = locale('zh-CN');
    const en = locale('en-US');
    expect(zh.clip.fullscreen).toBe('最大化');
    expect(zh.clip.exitFullscreen).toBe('退出最大化');
    expect(en.clip.fullscreen).toBe('Maximize');
    expect(en.clip.exitFullscreen).toBe('Exit maximize');
  });
});
