import { describe, expect, test } from 'bun:test';
import { ambientCanvasSize, mountAmbientCanvas, type AmbientPalette } from './ambientCanvas';

const { JSDOM } = require('jsdom') as {
  JSDOM: new (html: string) => { window: Window & typeof globalThis };
};

function setup(fps: number, maxPixels: number) {
  const dom = new JSDOM('<div id="wall"><canvas></canvas></div>');
  const view = dom.window;
  const { document } = view;
  const wall = document.querySelector<HTMLDivElement>('#wall')!;
  const canvas = document.querySelector('canvas')!;
  let rect = new view.DOMRect(0, 0, 1920, 1080);
  canvas.getBoundingClientRect = () => rect;

  let hidden = false;
  let accent = 'rgb(10, 40, 80)';
  const createMediaQuery = () => {
    const query = Object.assign(new view.EventTarget(), {
      matches: false,
      set(next: boolean) {
        query.matches = next;
        query.dispatchEvent(new view.Event('change'));
      },
    });
    return query;
  };
  const motion = createMediaQuery();
  const forcedColors = createMediaQuery();
  const frames = new Map<number, FrameRequestCallback>();
  const timers = new Map<number, { due: number; callback: () => void }>();
  let nextId = 0;
  let now = 0;
  let frameCalls = 0;
  let onResize: ResizeObserverCallback | undefined;
  Object.defineProperty(document, 'hidden', { get: () => hidden });
  Object.defineProperty(view, 'devicePixelRatio', { value: 1.5 });
  Object.defineProperty(view.performance, 'now', { value: () => now });
  Object.defineProperty(view, 'ResizeObserver', {
    value: class {
      constructor(callback: ResizeObserverCallback) { onResize = callback; }
      observe() {}
      disconnect() { onResize = undefined; }
    },
  });
  Object.defineProperty(view, 'matchMedia', {
    value: (query: string) => query.includes('forced-colors') ? forcedColors : motion,
  });
  view.requestAnimationFrame = (callback) => {
    frames.set(++nextId, callback);
    return nextId;
  };
  view.cancelAnimationFrame = (id) => { frames.delete(id); };
  Object.defineProperty(view, 'setTimeout', {
    value(callback: () => void, delay: number) {
      timers.set(++nextId, { due: now + Math.max(0, Math.floor(delay)), callback });
      return nextId;
    },
  });
  Object.defineProperty(view, 'clearTimeout', { value: (id: number) => timers.delete(id) });
  Object.defineProperty(view, 'getComputedStyle', { value: () => ({ color: accent }) });
  Object.defineProperty(view.HTMLCanvasElement.prototype, 'getContext', {
    value: () => ({
      fillStyle: '',
      clearRect() {},
      fillRect() {},
      getImageData() {
        return { data: [...(this.fillStyle.match(/\d+/g) ?? []).map(Number), 255] };
      },
    }),
  });

  const times: number[] = [];
  const palettes: AmbientPalette[] = [];
  const resized: DOMRect[] = [];
  const backingWrites: number[] = [];
  const drawnScales: Array<[number, number]> = [];
  let painted = false;
  let scale: [number, number] = [1, 1];
  const resetBitmap = () => {
    painted = false;
    scale = [1, 1];
  };
  for (const axis of ['width', 'height'] as const) {
    const descriptor = Object.getOwnPropertyDescriptor(view.HTMLCanvasElement.prototype, axis)!;
    Object.defineProperty(canvas, axis, {
      get() { return descriptor.get!.call(canvas); },
      set(value: number) {
        descriptor.set!.call(canvas, value);
        backingWrites.push(canvas.width * canvas.height);
        resetBitmap();
      },
    });
  }
  const context = {
    setTransform(x: number, _b: number, _c: number, y: number) { scale = [x, y]; },
  } as unknown as CanvasRenderingContext2D;
  const animation = mountAmbientCanvas(wall, canvas, context, {
    fps,
    maxPixels,
    resize(next) { resized.push(next); },
    setPalette(palette) { palettes.push(palette); },
    draw(time) {
      times.push(time);
      drawnScales.push(scale);
      painted = true;
    },
  });
  return {
    animation, backingWrites, canvas, document, drawnScales, forcedColors, motion, palettes, resized, times,
    get frameCalls() { return frameCalls; },
    get painted() { return painted; },
    get pendingWork() { return frames.size + timers.size; },
    step(time: number, frameTimestamp = time) {
      for (const [id, timer] of timers) {
        if (timer.due > time) continue;
        timers.delete(id);
        now = timer.due;
        timer.callback();
      }
      now = time;
      const ready = [...frames.values()];
      frames.clear();
      for (const frame of ready) {
        frameCalls += 1;
        frame(frameTimestamp);
      }
    },
    resizeTo(width: number, height: number) {
      rect = new view.DOMRect(0, 0, width, height);
      onResize?.([], {} as ResizeObserver);
    },
    loseContext() {
      resetBitmap();
      canvas.dispatchEvent(new view.Event('contextlost', { cancelable: true }));
    },
    restoreContext() {
      canvas.dispatchEvent(new view.Event('contextrestored'));
    },
    setHidden(next: boolean) {
      hidden = next;
      document.dispatchEvent(new view.Event('visibilitychange'));
    },
    setAccent(next: string) { accent = next; },
    dispose() {
      animation.dispose();
      dom.window.close();
    },
  };
}

describe('homepage ambient canvas', () => {
  test.each([{ fps: 15, maxPixels: 300_000 }, { fps: 20, maxPixels: 500_000 }])('caps actual draws and backing pixels at high refresh rates', ({ fps, maxPixels }) => {
    const fixture = setup(fps, maxPixels);
    try {
      fixture.step(0);
      for (let tick = 1; tick <= 120; tick += 1) fixture.step(tick * 1000 / 120);
      expect(fixture.times).toHaveLength(fps + 1);
      expect(fixture.frameCalls).toBe(fixture.times.length);
      expect(fixture.canvas.width * fixture.canvas.height).toBeLessThanOrEqual(maxPixels);
      expect(fixture.palettes).toHaveLength(1);
    } finally {
      fixture.dispose();
    }
    expect(fixture.pendingWork).toBe(0);
  });

  test('refresh timestamp jitter does not add empty frame callbacks or advance the deadline', () => {
    const fixture = setup(15, 300_000);
    try {
      fixture.step(0);
      for (let tick = 1; tick <= 120; tick += 1) {
        const clock = tick * 1000 / 120;
        fixture.step(clock, clock - (tick % 5) * 0.9);
      }
      expect(fixture.times).toHaveLength(16);
      expect(fixture.frameCalls).toBe(fixture.times.length);
    } finally {
      fixture.dispose();
    }
  });

  test('keeps the previous frame visible during a burst of resize notifications', () => {
    const fixture = setup(20, 500_000);
    try {
      fixture.step(0);
      const initialWrites = fixture.backingWrites.length;
      for (let tick = 1; tick <= 60; tick += 1) {
        fixture.step(tick * 1000 / 60);
        fixture.resizeTo(1920 + tick * 2, 1080);
        expect(fixture.painted).toBe(true);
      }
      fixture.step(1100);
      expect(fixture.resized.at(-1)?.width).toBe(2040);
      expect(fixture.backingWrites.length - initialWrites).toBeLessThanOrEqual(42);
    } finally {
      fixture.dispose();
    }
  });

  test('keeps every backing size in budget when switching from portrait to landscape', () => {
    const fixture = setup(20, 500_000);
    try {
      fixture.resizeTo(900, 1920);
      fixture.step(0);
      fixture.resizeTo(1920, 900);
      fixture.step(50);
      expect(fixture.backingWrites.every((pixels) => pixels <= 500_000)).toBe(true);
      expect(fixture.painted).toBe(true);
    } finally {
      fixture.dispose();
    }
  });

  test.each([{ paused: false }, { paused: true }])('restores the transform and repaints at the same size after context loss', ({ paused }) => {
    const fixture = setup(20, 500_000);
    try {
      fixture.animation.setPaused(paused);
      fixture.step(0);
      const initialScale = fixture.drawnScales[0];
      expect(initialScale[0]).toBeLessThan(1);
      fixture.loseContext();
      expect(fixture.pendingWork).toBe(0);
      fixture.step(100);
      expect(fixture.times).toHaveLength(1);
      fixture.restoreContext();
      fixture.step(120);
      expect(fixture.drawnScales).toEqual([initialScale, initialScale]);
      expect(fixture.palettes).toHaveLength(2);
      if (paused) {
        fixture.step(220);
        expect(fixture.times).toEqual([0, 0]);
        expect(fixture.pendingWork).toBe(0);
      }
    } finally {
      fixture.dispose();
    }
  });

  test('waits for a measurable canvas when the layout is initially collapsed', () => {
    const fixture = setup(20, 500_000);
    try {
      fixture.resizeTo(0, 0);
      fixture.step(0);
      expect(fixture.times).toHaveLength(0);
      expect(fixture.pendingWork).toBe(0);
      fixture.resizeTo(1920, 1080);
      fixture.step(50);
      expect(fixture.painted).toBe(true);
    } finally {
      fixture.dispose();
    }
  });

  test('cancels background draws and resumes without advancing paused time', () => {
    const fixture = setup(20, 500_000);
    try {
      fixture.step(0);
      fixture.step(50);
      fixture.animation.setPaused(true);
      expect(fixture.pendingWork).toBe(0);
      fixture.animation.setPaused(false);
      fixture.step(1000);
      expect(fixture.times).toEqual([0, 50, 50]);

      fixture.setHidden(true);
      expect(fixture.pendingWork).toBe(0);
      fixture.setHidden(false);
      fixture.step(2000);
      expect(fixture.times.at(-1)).toBe(50);

      fixture.motion.set(true);
      expect(fixture.pendingWork).toBe(0);
      fixture.motion.set(false);
      fixture.forcedColors.set(true);
      expect(fixture.pendingWork).toBe(0);
    } finally {
      fixture.dispose();
    }
  });

  test('updates the palette in the same theme mode while reduced motion is active', async () => {
    const fixture = setup(15, 300_000);
    try {
      fixture.document.documentElement.dataset.theme = 'dark';
      fixture.motion.set(true);
      fixture.step(0);
      expect(fixture.pendingWork).toBe(0);

      const stylesheet = fixture.document.createElement('style');
      stylesheet.id = 'user-wallpaper-tokens';
      fixture.setAccent('rgb(70, 20, 160)');
      fixture.document.head.appendChild(stylesheet);
      stylesheet.textContent = ':root { --color-primary: oklch(0.5 0.2 300); }';
      await Promise.resolve();
      fixture.step(100);

      expect(fixture.palettes.map(({ dark, accent }) => [dark, accent])).toEqual([
        [true, [10, 40, 80]],
        [true, [70, 20, 160]],
      ]);
      expect(fixture.times).toEqual([0, 0]);
      expect(fixture.pendingWork).toBe(0);
    } finally {
      fixture.dispose();
    }
  });

  test('keeps the pixel budget on extreme aspect ratios and avoids upscaling small canvases', () => {
    const thin = ambientCanvasSize(1_000_000, 1, 2, 300_000);
    expect(thin.pixelWidth * thin.pixelHeight).toBeLessThanOrEqual(300_000);
    expect(ambientCanvasSize(320, 200, 1, 300_000)).toEqual({ pixelWidth: 320, pixelHeight: 200 });
  });
});
