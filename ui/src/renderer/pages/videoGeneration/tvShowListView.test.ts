import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { readTvShowListView, tvShowMenuItem } from './tvShowListView';

describe('readTvShowListView', () => {
  test('defaults to the sphere and only restores the thumbnail list explicitly', () => {
    expect(readTvShowListView(null)).toBe('sphere');
    expect(readTvShowListView(undefined)).toBe('sphere');
    expect(readTvShowListView('sphere')).toBe('sphere');
    expect(readTvShowListView('anything')).toBe('sphere');
    expect(readTvShowListView('grid')).toBe('grid');
  });
});

describe('tvShowMenuItem', () => {
  test('keeps the cover, title, and description for the sphere tile', () => {
    expect(
      tvShowMenuItem(
        { id: 7, title: '  码头  ', coverUrl: ' https://cdn.example/cover.webp ' },
        'Ada · 创作画布'
      )
    ).toEqual({
      videoId: 7,
      image: 'https://cdn.example/cover.webp',
      title: '码头',
      description: 'Ada · 创作画布',
    });
  });

  test('keeps a hover preview when the film has one', () => {
    expect(
      tvShowMenuItem(
        {
          id: 8,
          title: '夜航',
          coverUrl: 'https://cdn.example/cover.webp',
          previewUrl: ' https://cdn.example/preview.mp4 ',
        },
        'Ada'
      ).preview
    ).toBe('https://cdn.example/preview.mp4');
  });
});

describe('Flowy TV list view', () => {
  test('keeps both presentations and defaults the control to the sphere', () => {
    const panel = readFileSync(new URL('./components/TvShowPanel.tsx', import.meta.url), 'utf8');
    const zh = readFileSync(new URL('../../services/i18n/locales/zh-CN/videoGeneration.json', import.meta.url), 'utf8');
    const en = readFileSync(new URL('../../services/i18n/locales/en-US/videoGeneration.json', import.meta.url), 'utf8');
    expect(panel.includes('readTvShowListView')).toBe(true);
    expect(panel.includes('<InfiniteMenu')).toBe(true);
    expect(panel.includes("chooseListView(listView === 'sphere' ? 'grid' : 'sphere')")).toBe(true);
    expect(panel.includes("listView === 'grid' && displayed.length > 0")).toBe(true);
    expect(panel.includes('videoGeneration.tvShow.view.thumbnails')).toBe(true);
    expect(panel.includes('videoGeneration.tvShow.view.sphere')).toBe(true);
    expect(panel.includes('videoGeneration.tvShow.channel.action2video')).toBe(false);
    expect(panel.includes('zoom={1}\n') || panel.includes('zoom={1}\r\n')).toBe(true);
    const menu = readFileSync(new URL('./components/InfiniteMenu.tsx', import.meta.url), 'utf8');
    const menuCss = readFileSync(new URL('./components/InfiniteMenu.module.css', import.meta.url), 'utf8');
    expect(menu.includes('hoveringActive && !moving')).toBe(true);
    expect(menuCss.includes('object-fit: contain')).toBe(true);
    expect(menuCss.includes('-webkit-line-clamp: 3')).toBe(true);
    expect(zh.includes('"thumbnails": "切换缩略图"')).toBe(true);
    expect(zh.includes('"sphere": "切换球面"')).toBe(true);
    expect(en.includes('"thumbnails": "Switch to thumbnails"')).toBe(true);
    expect(en.includes('"sphere": "Switch to sphere"')).toBe(true);
  });
});
