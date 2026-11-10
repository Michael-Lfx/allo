import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const read = (relative: string) => readFileSync(new URL(relative, import.meta.url), 'utf8');
const guide = read('../GuidPage.tsx');
const wall = read('./CreateAmbientWall.tsx');

describe('create ambient wall contract', () => {
  test('uses the GPU light field with explicit performance and accessibility guards', () => {
    expect(wall).toContain('GodRays');
    expect(wall).toContain('maxPixelCount');
    expect(wall).toContain('minPixelRatio');
    expect(wall).toContain('prefers-reduced-motion');
    expect(wall).toContain('visibilitychange');
    expect(guide).toContain('<CreateAmbientWall />');
    expect(guide).toContain("data-ambient-variant={buildAmbientVariant}");
  });
});
