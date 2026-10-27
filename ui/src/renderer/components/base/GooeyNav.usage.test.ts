import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import GooeyNav from './GooeyNav';

const read = (url: string) => readFileSync(new URL(url, import.meta.url), 'utf8');

describe('GooeyNav call sites', () => {
  test('session home, the resource rail, video tabs, and channel pills use it', () => {
    const profile = read('../agent/TaskProfileSelector.tsx');
    const sider = read('../layout/Sider/index.tsx');
    const videoHome = read('../../pages/videoGeneration/index.tsx');
    const filters = read('../../pages/videoGeneration/components/FilterPills.tsx');
    const tabs = read('./SegmentedTabs.tsx');
    const canvasCard = read('../../pages/workshop/generation/GeneratorCard.tsx');

    expect(profile.includes('<GooeyNav')).toBe(true);
    expect(profile.includes("selection='radio'")).toBe(true);
    expect(sider.includes("<GooeyNav")).toBe(true);
    expect(sider.includes('t(\'knowledge.title\')')).toBe(true);
    expect(sider.includes('t(\'learning.title\')')).toBe(true);
    expect(sider.includes('t(\'cron.scheduledTasks\')')).toBe(true);
    expect(videoHome.includes('<GooeyNav')).toBe(true);
    expect(videoHome.includes('videoGeneration.list.tabs.tvShow')).toBe(true);
    expect(videoHome.includes('videoGeneration.list.tabs.recent')).toBe(true);
    expect(filters.includes('<GooeyNav')).toBe(true);
    expect(tabs.includes("variant = 'gooey'")).toBe(true);
    expect(canvasCard.includes("variant='classic'")).toBe(true);
  });

  test('tabs expose tab semantics and an empty selection draws no current item', () => {
    const html = renderToStaticMarkup(
      React.createElement(GooeyNav, {
        selection: 'tab',
        ariaLabel: '频道',
        activeIndex: -1,
        items: [{ label: '全部' }, { label: '一句话' }],
      })
    );
    expect(html.includes('role="tablist"')).toBe(true);
    expect(html.includes('role="tab"')).toBe(true);
    expect(html.includes('aria-selected="true"')).toBe(false);
    expect(html.includes('全部')).toBe(true);
    expect(html.includes('一句话')).toBe(true);
  });
});
