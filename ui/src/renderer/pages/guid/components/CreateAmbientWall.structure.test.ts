import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const read = (relative: string) => readFileSync(new URL(relative, import.meta.url), 'utf8');
const guide = read('../GuidPage.tsx');
const wall = read('./CreateAmbientWall.tsx');

describe('create ambient wall contract', () => {
  test('uses a bounded Canvas 2D light field across the homepage stages', () => {
    expect(wall).toContain("getContext('2d')");
    expect(wall).not.toMatch(/webgl|GodRays/i);
    expect(wall).toContain('mountAmbientCanvas');
    expect(wall).toContain('maxPixels: 300_000');
    expect(wall).toContain('fps: 15');
    expect(guide).toContain('<CreateAmbientWall />');
    expect(guide).toContain("data-ambient-variant={buildAmbientVariant}");
  });

  test('mounts a dedicated video ambient variant without changing the create/image paths', () => {
    expect(guide).toContain("data-testid='guid-video-ambient'");
    expect(guide).toContain("<CreateAmbientWall variant='video' paused={videoPreviewPlaying} />");
    expect(guide).toContain("onPreviewPlayingChange={setVideoPreviewPlaying}");
    expect(wall).toContain("type CreateAmbientVariant = 'create' | 'image' | 'video'");
    expect(wall).toContain('data-variant={variant}');
    expect(wall).toContain('palette.dark');
    expect(wall).toContain('palette.accent');
  });
});
