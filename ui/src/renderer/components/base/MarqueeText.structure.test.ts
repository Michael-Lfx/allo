import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const componentSource = readFileSync(new URL('./MarqueeText.tsx', import.meta.url), 'utf8');
const styleSource = readFileSync(new URL('./MarqueeText.module.css', import.meta.url), 'utf8');

describe('MarqueeText structure', () => {
  test('measures real overflow and observes both viewport and full text', () => {
    expect(componentSource.includes('ResizeObserver')).toBe(true);
    expect(componentSource.includes('measure.scrollWidth - viewport.clientWidth')).toBe(true);
    expect(componentSource.includes('observer.observe(viewport)')).toBe(true);
    expect(componentSource.includes('observer.observe(measure)')).toBe(true);
    expect(componentSource.includes('active ||')).toBe(true);
    expect(componentSource.includes('}, [overflowDistance, stopPlayback, text]);')).toBe(true);
  });

  test('uses a delayed single playback with a bounded return', () => {
    expect(componentSource.includes('MARQUEE_START_DELAY_MS')).toBe(true);
    expect(componentSource.includes('MARQUEE_END_PAUSE_MS')).toBe(true);
    expect(componentSource.includes('MARQUEE_RETURN_MS')).toBe(true);
    expect(componentSource.includes('setIsPlaying(false)')).toBe(true);
    expect(styleSource.includes('transform: translate3d(var(--marquee-offset')).toBe(true);
    expect(styleSource.includes('will-change: transform')).toBe(true);
  });

  test('keeps the full text accessible and honors reduced motion', () => {
    expect(componentSource.includes('aria-label={text}')).toBe(true);
    expect(componentSource.includes("className='sr-only'>{text}")).toBe(true);
    expect(componentSource.includes("matchMedia('(prefers-reduced-motion: reduce)')")).toBe(true);
    expect(styleSource.includes('@media (prefers-reduced-motion: reduce)')).toBe(true);
  });
});
