

import { describe, expect, test } from 'bun:test';
import { clampRectToDisplays, isReachableOnDisplays, nearestDisplay, placeResizedWindow } from './windowGeometry';

const MON = { x: 0, y: 0, width: 2880, height: 1800 };

describe('placeResizedWindow', () => {
  test('anchors the bottom edge and keeps the horizontal center', () => {
    const pos = placeResizedWindow({ x: 1000, y: 1000, width: 240, height: 320 }, { width: 320, height: 600 }, [MON]);
    // x: 1000 + (240-320)/2 = 960; y: 1000 + (320-600) = 720
    expect(pos).toEqual({ x: 960, y: 720 });
  });

  test('clamps into the monitor when the taller window would hang past an edge', () => {
    const pos = placeResizedWindow({ x: 100, y: 1700, width: 240, height: 320 }, { width: 320, height: 600 }, [MON]);
    // unclamped y = 1420 → bottom 2020 > 1800 → clamp to 1800-600 = 1200
    expect(pos).toEqual({ x: 60, y: 1200 });
  });

  test('clamps into the monitor with the largest overlap', () => {
    const left = { x: 0, y: 0, width: 1920, height: 1080 };
    const right = { x: 1920, y: 0, width: 1920, height: 1080 };
    const pos = placeResizedWindow({ x: 1900, y: 700, width: 240, height: 320 }, { width: 320, height: 600 }, [left, right]);
    // overlap: left 20px wide vs right 220px wide → right wins; x clamps to 1920, y = 700-280 = 420
    expect(pos).toEqual({ x: 1920, y: 420 });
  });

  test('returns the unclamped placement when no monitors are known', () => {
    const pos = placeResizedWindow({ x: 0, y: 0, width: 240, height: 320 }, { width: 320, height: 600 }, []);
    expect(pos).toEqual({ x: -40, y: -280 });
  });

  test('is a pure clamp when the size does not change', () => {
    const pos = placeResizedWindow({ x: 500, y: 500, width: 240, height: 320 }, { width: 240, height: 320 }, [MON]);
    expect(pos).toEqual({ x: 500, y: 500 });
  });

  test('handles monitors at negative coordinates', () => {
    const negMon = { x: -1920, y: -1080, width: 1920, height: 1080 };
    const pos = placeResizedWindow({ x: -400, y: -310, width: 240, height: 320 }, { width: 320, height: 600 }, [negMon]);
    // x: -400 + (240-320)/2 = -440 (within [-1920, -320]); y: -310-280 = -590 → clamp to -1080+1080-600 = -600
    expect(pos).toEqual({ x: -440, y: -600 });
  });

  test('pins the top edge when the window is taller than the monitor', () => {
    const shortMon = { x: 0, y: 0, width: 2880, height: 500 };
    const pos = placeResizedWindow({ x: 100, y: 100, width: 240, height: 320 }, { width: 320, height: 600 }, [shortMon]);
    // y unclamped = -180; clamp range degenerates to [0, 0] via Math.max(lo, hi) → top edge pinned
    expect(pos).toEqual({ x: 60, y: 0 });
  });

  test('falls back to the first monitor when the old rect overlaps nothing', () => {
    const mon = { x: 0, y: 0, width: 1920, height: 1080 };
    const pos = placeResizedWindow({ x: 5000, y: 5000, width: 240, height: 320 }, { width: 320, height: 600 }, [mon]);
    // zero overlap → monitors[0]; x: 4960 → 1600 (=1920-320); y: 4720 → 480 (=1080-600)
    expect(pos).toEqual({ x: 1600, y: 480 });
  });

  test('keeps the top-left when the old rect already has the new size (cold-start restore)', () => {
    const pos = placeResizedWindow({ x: 800, y: 900, width: 320, height: 600 }, { width: 320, height: 600 }, [MON]);
    expect(pos).toEqual({ x: 800, y: 900 });
  });

  test('clamps a same-size rect that hangs past the monitor bottom (cold-start restore)', () => {
    const pos = placeResizedWindow({ x: 1000, y: 800, width: 640, height: 1200 }, { width: 640, height: 1200 }, [MON]);
    // identical size → pure clamp; y bound = 1800 - 1200 = 600
    expect(pos).toEqual({ x: 1000, y: 600 });
  });
});

describe('clampRectToDisplays', () => {
  const left = { x: 0, y: 0, width: 1920, height: 1080 };
  const right = { x: 1920, y: 0, width: 1920, height: 1080 };
  const pet = { width: 240, height: 214 };

  test('leaves an on-screen rect untouched', () => {
    expect(clampRectToDisplays({ x: 100, y: 200, ...pet }, [MON])).toEqual({ x: 100, y: 200 });
  });

  test('pulls a window fully off the virtual desktop back onto the nearest display', () => {
    expect(clampRectToDisplays({ x: 8000, y: 4000, ...pet }, [left, right])).toEqual({
      x: 1920 + 1920 - 240,
      y: 1080 - 214,
    });
  });

  test('snaps onto the display with the largest overlap instead of straddling the bezel', () => {
    // 20px on the left, 220px on the right → right wins; x clamps to 1920
    expect(clampRectToDisplays({ x: 1900, y: 100, ...pet }, [left, right])).toEqual({ x: 1920, y: 100 });
  });

  test('handles a negatively placed secondary display', () => {
    const neg = { x: -1920, y: 0, width: 1920, height: 1080 };
    expect(clampRectToDisplays({ x: -4000, y: 40, ...pet }, [neg, left])).toEqual({ x: -1920, y: 40 });
  });

  test('pins the top-left when the window is taller than the host', () => {
    const short = { x: 0, y: 0, width: 1920, height: 200 };
    expect(clampRectToDisplays({ x: 10, y: -80, width: 240, height: 320 }, [short])).toEqual({ x: 10, y: 0 });
  });

  test('is a no-op when no displays are known', () => {
    expect(clampRectToDisplays({ x: -50, y: -50, ...pet }, [])).toEqual({ x: -50, y: -50 });
  });
});

describe('isReachableOnDisplays / nearestDisplay', () => {
  const left = { x: 0, y: 0, width: 1920, height: 1080 };
  const right = { x: 2000, y: 0, width: 1920, height: 1080 }; // 80px gap

  test('rejects a window parked in the gap between monitors', () => {
    expect(isReachableOnDisplays({ x: 1930, y: 100, width: 50, height: 50 }, [left, right])).toBe(false);
  });

  test('accepts a window that still has a grabable slice on a display', () => {
    expect(isReachableOnDisplays({ x: 1900, y: 100, width: 240, height: 214 }, [left, right])).toBe(true);
  });

  test('picks the geometrically nearest display across a gap', () => {
    expect(nearestDisplay({ x: 1940, y: 100, width: 40, height: 40 }, [left, right])).toEqual(left);
  });
});
