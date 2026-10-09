import { describe, expect, test } from 'bun:test';
import { clampSavedCompanionPosition, workAreasFromMonitors } from './companionScreenClamp';

describe('workAreasFromMonitors', () => {
  test('prefers a non-empty work area over the full display', () => {
    expect(
      workAreasFromMonitors([
        {
          position: { x: 0, y: 0 },
          size: { width: 1920, height: 1080 },
          workArea: { position: { x: 0, y: 0 }, size: { width: 1920, height: 1040 } },
        },
      ])
    ).toEqual([{ x: 0, y: 0, width: 1920, height: 1040 }]);
  });

  test('falls back to the display bounds when work area is missing or empty', () => {
    expect(
      workAreasFromMonitors([
        { position: { x: -1920, y: 0 }, size: { width: 1920, height: 1080 } },
        {
          position: { x: 0, y: 0 },
          size: { width: 800, height: 600 },
          workArea: { position: { x: 0, y: 0 }, size: { width: 0, height: 0 } },
        },
      ])
    ).toEqual([
      { x: -1920, y: 0, width: 1920, height: 1080 },
      { x: 0, y: 0, width: 800, height: 600 },
    ]);
  });

  test('clamps a saved companion fully into the overlapping work area', () => {
    const monitors = [
      {
        position: { x: 0, y: 0 },
        size: { width: 1920, height: 1080 },
        workArea: { position: { x: 0, y: 0 }, size: { width: 1920, height: 1040 } },
      },
      {
        position: { x: 1920, y: 0 },
        size: { width: 1920, height: 1080 },
        workArea: { position: { x: 1920, y: 0 }, size: { width: 1920, height: 1040 } },
      },
    ];
    expect(clampSavedCompanionPosition({ x: 8000, y: 4000 }, { width: 240, height: 214 }, monitors)).toEqual({
      x: 1920 + 1920 - 240,
      y: 1040 - 214,
    });
    expect(clampSavedCompanionPosition({ x: 1900, y: 100 }, { width: 240, height: 214 }, monitors)).toEqual({
      x: 1920,
      y: 100,
    });
  });
});
