import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const read = (relative: string) => readFileSync(new URL(relative, import.meta.url), 'utf8');
const guide = read('../GuidPage.tsx');
const wall = read('./CreateAmbientWall.tsx');
const styles = read('./CreateAmbientWall.module.css');

describe('create ambient wall contract', () => {
  test('draws sparse Canvas 2D patterns in a bounded surface', () => {
    expect(wall).toContain("getContext('2d')");
    expect(wall).not.toMatch(/webgl|GodRays|drawImage|createRadialGradient/i);
    expect(wall).toContain('mountAmbientCanvas');
    expect(wall).toContain('maxPixels: 48_000');
    expect(wall).toContain('fps: 10');
    expect(styles).toContain('width: min(100%, 720px)');
    expect(styles).toContain('height: clamp(64px, 14vh, 112px)');
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
