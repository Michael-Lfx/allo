import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { nextShotCanvasWheelArmed } from './shotCanvasWheel';

const here = dirname(fileURLToPath(import.meta.url));

describe('storyboard canvas wheel arming', () => {
  test('arms on a canvas click and releases on an outside click', () => {
    expect(nextShotCanvasWheelArmed('canvas', false)).toBe(true);
    expect(nextShotCanvasWheelArmed('portal', true)).toBe(true);
    expect(nextShotCanvasWheelArmed('portal', false)).toBe(false);
    expect(nextShotCanvasWheelArmed('outside', true)).toBe(false);
  });

  test('storyboard canvas waits for a click; the full canvas page does not', () => {
    const shot = readFileSync(join(here, 'ShotInfiniteCanvas.tsx'), 'utf8');
    const stage = readFileSync(join(here, '../../videoCanvas/oc/pages/canvas/canvas-project-stage.tsx'), 'utf8');
    const infinite = readFileSync(join(here, '../../videoCanvas/oc/components/canvas/infinite-canvas.tsx'), 'utf8');

    expect(shot.includes('wheelGestures={expanded || wheelArmed}')).toBe(true);
    expect(shot.includes('shotCanvasPointerHit')).toBe(true);
    expect(stage.includes('wheelGestures')).toBe(false);
    expect(infinite.includes('forwardWheelToScrollableAncestor')).toBe(true);
    expect(infinite.includes('wheelGestures = true')).toBe(true);
  });
});
