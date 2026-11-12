import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const read = (relative: string) => readFileSync(new URL(relative, import.meta.url), 'utf8');
const guide = read('../GuidPage.tsx');
const wall = read('./CreateAmbientWall.tsx');
const styles = read('./CreateAmbientWall.module.css');
const scene = read('./ambientCreateScene.ts');
const engine = read('./ambientCanvas.ts');

describe('create ambient wall contract', () => {
  test('renders one full-bleed bounded 2D scene without CSS or GPU effects', () => {
    expect(wall.match(/<canvas\b/g)).toHaveLength(1);
    expect(wall).toContain("getContext('2d', { alpha: true, desynchronized: true })");
    expect(wall).toContain('mountAmbientCanvas(wall, canvas, context');
    expect(wall).toContain('...CREATE_AMBIENT_BUDGET');
    expect(wall).toContain("data-testid='guid-create-ambient'");
    expect(wall).toContain('data-variant={variant}');
    expect(wall).toContain('data-paused={paused');
    expect(styles).toContain('inset: 0');
    expect(styles).toContain('width: 100%');
    expect(styles).toContain('height: 100%');
    expect(styles).not.toContain('width: min(100%, 720px)');
    expect(styles).toContain("[data-variant='image']");
    expect(styles).toContain("[data-variant='video']");
    expect(styles).toContain('radial-gradient');
    expect(styles).toContain("data-theme='light'");
    expect(styles).toContain('forced-colors');
    expect(styles).not.toMatch(/@keyframes|\banimation\s*:|conic-gradient|\bfilter\s*:|backdrop-filter|mix-blend-mode|will-change|url\(/);
    expect(wall + scene).not.toMatch(/webgl|GodRays|drawImage|shadowBlur|\.filter\s*=|getImageData/i);
  });

  test('delegates reduced motion, playback, visibility and context recovery to the shared scheduler', () => {
    expect(wall).toContain('setPaused(paused)');
    expect(wall).toContain('[paused, variant]');
    expect(wall).toContain('animation.dispose()');
    expect(engine).toContain('prefers-reduced-motion: reduce');
    expect(engine).toContain('forced-colors: active');
    expect(engine).toContain('!doc.hidden');
    expect(engine).toContain("addEventListener('visibilitychange'");
    expect(engine).toContain("addEventListener('contextlost'");
    expect(engine).toContain("addEventListener('contextrestored'");
    expect(wall).not.toMatch(/requestAnimationFrame|setInterval|setTimeout|useState|\.style\./);
  });

  test('pointer updates stay in canvas memory and every listener is cleaned up', () => {
    expect(wall).toContain("view.addEventListener('pointermove', onMove, { passive: true })");
    expect(wall).toContain("wall.dataset.paused === 'true'");
    expect(wall).toContain('!motion.matches || !fine.matches || colors.matches');
    expect(wall).toContain('scene.setPointer(x, y)');
    expect(wall).toContain('scene.resetPointer()');
    expect(wall).not.toContain('getBoundingClientRect');
    for (const [event, callback] of [
      ['pointermove', 'onMove'], ['pointerleave', 'reset'], ['blur', 'reset'], ['visibilitychange', 'reset'],
    ]) {
      expect(wall).toContain(`addEventListener('${event}', ${callback}`);
      expect(wall).toContain(`removeEventListener('${event}', ${callback}`);
    }
    for (const media of ['motion', 'fine', 'colors']) {
      expect(wall).toContain(`${media}.addEventListener('change', reset)`);
      expect(wall).toContain(`${media}.removeEventListener('change', reset)`);
    }
  });

  test('keeps the create, image and preview-paused video call sites intact', () => {
    expect(guide).toContain("data-testid='guid-video-ambient'");
    expect(guide).toContain('<CreateAmbientWall />');
    expect(guide).toContain("<CreateAmbientWall variant='image' />");
    expect(guide).toContain("<CreateAmbientWall variant='video' paused={videoPreviewPlaying} />");
    expect(guide).toContain('onPreviewPlayingChange={setVideoPreviewPlaying}');
    expect(scene).toContain("type CreateAmbientVariant = 'create' | 'image' | 'video'");
    expect(wall).toContain('aria-hidden');
    expect(styles).toContain('pointer-events: none');
  });
});
