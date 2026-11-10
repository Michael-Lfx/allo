import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const read = (relative: string) => readFileSync(new URL(relative, import.meta.url), 'utf8');
const guide = read('../GuidPage.tsx');
const wall = read('./VideoExampleWall.tsx');

describe('homepage video example wall contract', () => {
  test('renders the video capability as a data-backed example wall', () => {
    expect(guide).toContain('<VideoExampleWall');
    expect(guide).toContain('onOpenAll={() => navigate(\'/video-generation\')}');
    expect(guide).toContain('data-video-gallery=');
    expect(guide).not.toContain('guid-home-video-hero');
    expect(wall).toContain('listTvShow');
    expect(wall).toContain('pageSize: PAGE_SIZE');
    expect(wall).toContain('guid-home-video-examples');
    expect(wall).toContain('previewUrl');
    expect(wall).toContain('MAX_EXAMPLES');
    expect(wall).toContain('!loading && error && videos.length === 0');
    expect(wall).toContain('videos.length > 0 ?');
    expect(wall).toContain('onProgress');
    expect(wall).toContain('prefers-reduced-motion');
  });
});
