import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');

describe('hover preview returns to cover', () => {
  test('hover video is hidden unless Visible; always-on preview keeps its own class', () => {
    const css = source('./index.module.css');
    const videoRule = css.slice(css.indexOf('.projectCoverVideo {'));
    const hiddenDefault = videoRule.slice(0, videoRule.indexOf('.projectCoverVideoVisible'));
    expect(hiddenDefault).toContain('opacity: 0');
    expect(css).toContain('.projectCoverVideoVisible');
    expect(css).toContain('.projectCoverVideoAlwaysOn');
  });

  test('community and recent cards bind poster and do not seek while leaving', () => {
    const tv = source('./components/TvShowCard.tsx');
    const recent = source('./components/SessionCard.tsx');
    expect(tv).toContain('poster={video.coverUrl');
    expect(tv).toContain('projectCoverVideoVisible');
    expect(tv).toContain('node.paused');
    expect(recent).toContain('poster={coverUrl');
    expect(recent).toContain('projectCoverVideoVisible');
    expect(recent).toContain('node.paused');
  });

  test('generation task preview stays visible without hover', () => {
    const card = source('./components/GenerationTaskCard.tsx');
    expect(card).toContain('projectCoverVideoAlwaysOn');
  });
});
