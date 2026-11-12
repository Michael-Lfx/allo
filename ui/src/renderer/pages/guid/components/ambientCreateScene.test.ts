import { describe, expect, test } from 'bun:test';
import { createHash } from 'node:crypto';
import { createAmbientScene, CREATE_AMBIENT_BUDGET, type CreateAmbientVariant } from './ambientCreateScene';
import type { AmbientPalette } from './ambientCanvas';

const palette: AmbientPalette = {
  dark: true, surface: [28, 28, 30], accent: [133, 119, 204], ink: [239, 238, 245],
};
const lightPalette: AmbientPalette = {
  dark: false, surface: [245, 245, 247], accent: [60, 68, 91], ink: [29, 29, 31],
};
const variants: CreateAmbientVariant[] = ['create', 'image', 'video'];

function fixture(variant: CreateAmbientVariant) {
  const calls: Array<{ method: string; args: number[]; alpha: number; color: string; lineWidth: number }> = [];
  const context = { globalAlpha: 1, fillStyle: '', strokeStyle: '', lineWidth: 1 } as CanvasRenderingContext2D;
  const states: Array<Pick<CanvasRenderingContext2D, 'globalAlpha' | 'fillStyle' | 'strokeStyle' | 'lineWidth'>> = [];
  let gradientCount = 0;
  context.createLinearGradient = () => {
    gradientCount += 1;
    const stops: string[] = [];
    return { addColorStop: (offset: number, color: string) => stops.push(`${offset}:${color}`), toString: () => stops.join(';') } as unknown as CanvasGradient;
  };
  for (const method of ['clearRect', 'save', 'restore', 'beginPath', 'closePath', 'moveTo', 'lineTo', 'bezierCurveTo', 'arc', 'rect', 'fill', 'stroke', 'translate', 'rotate', 'transform']) {
    Object.assign(context, {
      [method](...args: number[]) {
        calls.push({ method, args, alpha: context.globalAlpha, color: String(method === 'fill' ? context.fillStyle : context.strokeStyle), lineWidth: context.lineWidth });
        if (method === 'save') {
          const { globalAlpha, fillStyle, strokeStyle, lineWidth } = context;
          states.push({ globalAlpha, fillStyle, strokeStyle, lineWidth });
        } else if (method === 'restore') Object.assign(context, states.pop());
      },
    });
  }
  const scene = createAmbientScene(context, variant);
  scene.resize(1440, 900);
  scene.setPalette(palette);
  return {
    scene,
    get gradientCount() { return gradientCount; },
    draw(time = 1000, delta = 50) {
      calls.length = 0;
      scene.draw(time, delta);
      return calls.map((call) => ({ ...call, args: [...call.args] }));
    },
  };
}

function ribbonPaths(calls: ReturnType<ReturnType<typeof fixture>['draw']>) {
  let path: typeof calls = [];
  let start: number[] = [];
  const paths: Array<{ start: number[]; curves: number[][]; color: string }> = [];
  for (const call of calls) {
    if (call.method === 'beginPath') path = [];
    else if (call.method === 'moveTo') start = call.args;
    else if (call.method === 'bezierCurveTo') path.push(call);
    else if ((call.method === 'fill' || call.method === 'stroke') && path.length) {
      paths.push({ start, curves: path.map(({ args }) => args), color: call.color });
    }
  }
  return paths;
}

describe('creative ambient scenes', () => {
  test('uses the approved scheduler budget', () => {
    expect(CREATE_AMBIENT_BUDGET).toEqual({ maxPixels: 500_000, fps: 20 });
  });

  test.each(variants)('%s stays deterministic, finite and within its draw complexity budget', (variant) => {
    const a = fixture(variant);
    const b = fixture(variant);
    const calls = a.draw();
    expect(calls).toEqual(b.draw());
    expect(calls.filter(({ method }) => method === 'clearRect').map(({ args }) => args)).toEqual([[0, 0, 1440, 900]]);
    expect(calls.every(({ args }) => args.every(Number.isFinite))).toBe(true);
    const budget = variant === 'create' ? { stroke: 21, lineTo: 93, bezierCurveTo: 576, fill: 39 }
      : variant === 'image' ? { stroke: 200, lineTo: 700, fill: 230 }
        : { stroke: 45, lineTo: 858, fill: 3 };
    for (const [method, limit] of Object.entries(budget)) {
      expect(calls.filter((call) => call.method === method).length).toBeLessThanOrEqual(limit);
    }
    expect(calls.every(({ alpha }) => alpha >= 0 && alpha <= 1)).toBe(true);
    expect(calls.some(({ method }) => method === 'stroke')).toBe(true);
    expect(a.draw(3000)).not.toEqual(calls);
  });

  test.each(variants)('%s responds to the pointer and resets without persistent distortion', (variant) => {
    const f = fixture(variant);
    const initial = f.draw();
    f.scene.setPointer(0.22, 0.2);
    expect(f.draw()).not.toEqual(initial);
    f.scene.resetPointer();
    expect(f.draw()).toEqual(initial);
    f.scene.setPointer(-10, 10);
    const clamped = f.draw();
    f.scene.resetPointer();
    f.scene.setPointer(0, 1);
    expect(f.draw()).toEqual(clamped);
  });

  test('create changes its silhouette with time and pointer, then restores a stable frozen frame', () => {
    const f = fixture('create');
    const initial = f.draw(0, 0);
    const contour = (calls: ReturnType<typeof f.draw>) => calls
      .filter(({ method }) => method === 'moveTo' || method === 'bezierCurveTo')
      .map(({ args }) => args.slice(-2));
    const origin = contour(initial);
    const shift = (calls: ReturnType<typeof f.draw>) => Math.max(...contour(calls).map(([x, y], index) => {
      return Math.hypot(x - origin[index][0], y - origin[index][1]);
    }));
    expect(shift(f.draw(6000, 0))).toBeGreaterThan(24);
    // Time is owned by mountAmbientCanvas, never accumulated inside the scene.
    expect(f.draw(0, 0)).toEqual(initial);
    f.scene.setPointer(0.52, 0.33);
    expect(shift(f.draw(0, 1000))).toBeGreaterThan(8);
    f.scene.resetPointer();
    expect(f.draw(0, 0)).toEqual(initial);
    f.scene.setPointer(0.6, 0.96);
    expect(f.draw(0, 1000)).toEqual(initial);
  });

  test('create shares complete cubic boundaries and keeps tangent continuity without panel seams', () => {
    const f = fixture('create');
    for (const time of [0, 4000, 8000, 17999, 18000, 30000]) {
      const [base, left, right, ...edges] = ribbonPaths(f.draw(time, 0));
      const steps = edges[0].curves.length;
      expect(base.curves).toHaveLength(steps * 2);
      expect(left.curves.slice(0, steps)).toEqual(edges[0].curves);
      expect(right.curves.slice(0, steps)).toEqual(edges[1].curves);
      expect(left.curves.slice(steps)).toEqual(edges[1].curves.map((_, i) => {
        const index = steps - 1 - i;
        const [x1, y1, x2, y2] = edges[1].curves[index];
        const [x, y] = index ? edges[1].curves[index - 1].slice(-2) : edges[1].start;
        return [x2, y2, x1, y1, x, y];
      }));
      expect(right.curves.slice(steps)).toEqual(base.curves.slice(steps));
      for (const { curves } of edges) {
        for (let i = 1; i < curves.length; i += 1) {
          const [,, cx, cy, x, y] = curves[i - 1];
          const [nx, ny] = curves[i];
          expect(nx - x).toBeCloseTo(x - cx, 8);
          expect(ny - y).toBeCloseTo(y - cy, 8);
          expect(curves[i][4]).toBeGreaterThan(x);
        }
      }
      for (let i = 0; i < steps; i += 1) {
        for (const t of [0.25, 0.5, 0.75]) {
          const ys = edges.map(({ curves, start }) => {
            const y = i ? curves[i - 1][5] : start[1];
            const [, c1,, c2,, end] = curves[i];
            return (1 - t) ** 3 * y + 3 * (1 - t) ** 2 * t * c1 + 3 * (1 - t) * t * t * c2 + t ** 3 * end;
          });
          expect(ys[1]).toBeGreaterThanOrEqual(Math.min(ys[0], ys[2]) - 0.01);
          expect(ys[1]).toBeLessThanOrEqual(Math.max(ys[0], ys[2]) + 0.01);
        }
      }
    }
  });

  test('create varies surface color continuously through turns and at adjacent frames', () => {
    const f = fixture('create');
    const colors = (time: number) => ribbonPaths(f.draw(time, 0)).slice(1, 3).map(({ color }) =>
      color.split(';').map((stop) => stop.match(/rgba\((\d+), (\d+), (\d+), 1\)/)!.slice(1).map(Number)),
    );
    for (const theme of [palette, lightPalette]) {
      f.scene.setPalette(theme);
      for (const time of [0, 4000, 8000, 12499, 17999, 30000]) {
        const current = colors(time);
        const next = colors(time + 50);
        for (let face = 0; face < 2; face += 1) {
          for (let i = 1; i < current[face].length; i += 1) {
            for (let channel = 0; channel < 3; channel += 1) {
              expect(Math.abs(current[face][i][channel] - current[face][i - 1][channel])).toBeLessThanOrEqual(10);
              expect(Math.abs(next[face][i][channel] - current[face][i][channel])).toBeLessThanOrEqual(3);
            }
          }
        }
      }
    }
  });

  test('create has no time wrap jump and pointer deformation stays local to the ribbon', () => {
    const f = fixture('create');
    const geometry = (time: number, delta = 0) => ribbonPaths(f.draw(time, delta)).slice(3).flatMap(({ curves }) => curves);
    for (const time of [0, 4000, 8000, 12499, 12500, 17999, 18000, 120000]) {
      const a = geometry(time);
      const b = geometry(time + 50);
      expect(Math.max(...a.flatMap((args, i) => args.map((value, j) => Math.abs(value - b[i][j]))))).toBeLessThan(3);
    }
    const initial = geometry(0);
    f.scene.setPointer(0.52, 0.33);
    const focused = geometry(0, 1000);
    let moved = 0;
    for (let i = 0; i < initial.length; i += 1) {
      if (initial[i][4] < 1440 * 0.25 || initial[i][4] > 1440 * 0.8) expect(focused[i]).toEqual(initial[i]);
      if (focused[i][5] !== initial[i][5]) moved += 1;
    }
    expect(moved).toBeGreaterThan(0);
  });

  test.each(variants)('%s adapts geometry and colors to narrow/light surfaces without advancing time', (variant) => {
    const f = fixture(variant);
    const initial = f.draw(0, 0);
    f.scene.resize(390, 780);
    const narrow = f.draw(0, 0);
    expect(narrow[0].args).toEqual([0, 0, 390, 780]);
    expect(narrow.filter(({ method }) => method === 'moveTo')).not.toEqual(initial.filter(({ method }) => method === 'moveTo'));
    f.scene.setPalette(lightPalette);
    const light = f.draw(0, 0);
    expect(light.filter(({ method }) => method === 'moveTo').map(({ args }) => args)).toEqual(
      narrow.filter(({ method }) => method === 'moveTo').map(({ args }) => args),
    );
    expect(light.filter(({ method }) => method === 'stroke').map(({ color }) => color)).not.toEqual(
      narrow.filter(({ method }) => method === 'stroke').map(({ color }) => color),
    );
  });

  test('preserves the approved image drawing commands across 72 baseline frames', () => {
    const f = fixture('image');
    const frames = [];
    for (const theme of [palette, lightPalette]) {
      f.scene.setPalette(theme);
      for (const [width, height] of [[1440, 900], [880, 780], [390, 780]]) {
        f.scene.resize(width, height);
        for (const time of [0, 1000, 30000]) {
          f.scene.resetPointer();
          frames.push(f.draw(time));
          f.scene.setPointer(0.22, 0.2);
          frames.push(f.draw(time));
          f.scene.setPointer(0.6, 0.245);
          frames.push(f.draw(time, 1000));
          f.scene.resetPointer();
          frames.push(f.draw(time));
        }
      }
    }
    expect(frames).toHaveLength(72);
    // Capture the approved round-two commands before changing only create.
    expect(createHash('sha256').update(JSON.stringify(frames)).digest('hex')).toBe(
      '8c5e51ae1f93742c1bdf8ae14791ac0d51c4a1ac0fad99f7254964fff10787d8',
    );
    expect(f.gradientCount).toBe(3);
  });

  test('preserves the approved video drawing commands across 54 baseline frames', () => {
    const f = fixture('video');
    const frames = [];
    for (const theme of [palette, lightPalette]) {
      f.scene.setPalette(theme);
      for (const [width, height] of [[1440, 900], [880, 780], [390, 780]]) {
        f.scene.resize(width, height);
        for (const time of [0, 1000, 30000]) {
          f.scene.resetPointer();
          frames.push(f.draw(time));
          f.scene.setPointer(0.22, 0.2);
          frames.push(f.draw(time));
          f.scene.resetPointer();
          frames.push(f.draw(time));
        }
      }
    }
    expect(frames).toHaveLength(54);
    // Captured from round one before replacing the create/image implementations.
    expect(createHash('sha256').update(JSON.stringify(frames)).digest('hex')).toBe(
      '91dc6b4220ba7831f7c68ced769db8a692a619568d4d311d0e810c05e941815b',
    );
    expect(f.gradientCount).toBe(0);
  });

  test.each(['create', 'image'] as const)('%s bounds materials and keeps the lower input region free of geometry', (variant) => {
    const f = fixture(variant);
    expect(f.gradientCount).toBe(1);
    let frames = 0;
    for (const [width, height] of [[1440, 900], [880, 780], [390, 780]]) {
      f.scene.resize(width, height);
      const times = variant === 'create' ? [0, 1000, 4000, 8000, 12499, 12500, 30000, 120000] : [0, 1000, 30000];
      for (const time of times) {
        const targets = variant === 'create' ? [[0, 0], [0.5, 0.33], [1, 0.55], [0.65, 0.96]] : [[0.65, 0.3]];
        for (const [x, y] of targets) {
          f.scene.resetPointer();
          f.scene.setPointer(x, y);
          const calls = f.draw(time, 1000);
          frames += 1;
          expect(calls.every(({ args, alpha }) => args.every(Number.isFinite) && alpha >= 0 && alpha <= 1)).toBe(true);
          expect(calls.filter(({ method }) => method === 'clearRect')).toHaveLength(1);
          if (variant === 'create') {
            for (const [method, limit] of Object.entries({ fill: 39, stroke: 21, lineTo: 93, bezierCurveTo: 576 })) {
              expect(calls.filter((call) => call.method === method).length).toBeLessThanOrEqual(limit);
            }
          }
          let matrix = [1, 0, 0, 1, 0, 0];
          const stack: number[][] = [];
          const multiply = ([a, b, c, d, e, g]: number[]) => {
            const [ma, mb, mc, md, me, mg] = matrix;
            matrix = [ma * a + mc * b, mb * a + md * b, ma * c + mc * d,
              mb * c + md * d, ma * e + mc * g + me, mb * e + md * g + mg];
          };
          const ys: number[] = [];
          const point = (x: number, y: number) => ys.push(matrix[1] * x + matrix[3] * y + matrix[5]);
          for (const { method, args } of calls) {
            if (method === 'save') stack.push([...matrix]);
            else if (method === 'restore') matrix = stack.pop()!;
            else if (method === 'translate') multiply([1, 0, 0, 1, args[0], args[1]]);
            else if (method === 'rotate') multiply([Math.cos(args[0]), Math.sin(args[0]), -Math.sin(args[0]), Math.cos(args[0]), 0, 0]);
            else if (method === 'transform') multiply(args);
            else if (method === 'moveTo' || method === 'lineTo') point(args[0], args[1]);
            else if (method === 'bezierCurveTo') {
              point(args[0], args[1]); point(args[2], args[3]); point(args[4], args[5]);
            }
            else if (method === 'rect' || method === 'arc') {
              const [x, y, w, h] = method === 'rect' ? args : [args[0] - args[2], args[1] - args[2], args[2] * 2, args[2] * 2];
              point(x, y); point(x + w, y); point(x, y + h); point(x + w, y + h);
            }
          }
          expect(Math.max(...ys)).toBeLessThan(height * 0.58);
          expect(stack).toHaveLength(0);
        }
      }
    }
    expect(f.gradientCount).toBe(1 + (variant === 'create' ? frames * 2 : 0));
    f.scene.setPalette(lightPalette);
    expect(f.gradientCount).toBe(2 + (variant === 'create' ? frames * 2 : 0));
  });

  test('image focus reveals local texture without moving the exposure plates', () => {
    const f = fixture('image');
    const initial = f.draw(0, 0);
    f.scene.setPointer(0.6, 0.245);
    const focused = f.draw(0, 1000);
    for (const method of ['rect', 'translate', 'transform']) {
      expect(focused.filter((call) => call.method === method).map(({ args }) => args)).toEqual(
        initial.filter((call) => call.method === method).map(({ args }) => args),
      );
    }
    expect(focused.filter(({ method }) => method === 'stroke').length).toBeGreaterThan(
      initial.filter(({ method }) => method === 'stroke').length,
    );
    f.scene.resetPointer();
    f.scene.setPointer(0.6, 0.96);
    expect(f.draw(0, 1000)).toEqual(initial);
  });
});
