import { describe, expect, test } from 'bun:test';
import { canScheduleCanvasGenerationBatches } from './lib/modelCatalogReadiness';

describe('video canvas model catalog readiness', () => {
  test('does not schedule persisted waiting items until the catalog has synced', () => {
    expect(canScheduleCanvasGenerationBatches(true, false)).toBe(false);
    expect(canScheduleCanvasGenerationBatches(false, true)).toBe(false);
    expect(canScheduleCanvasGenerationBatches(false, false)).toBe(false);
    expect(canScheduleCanvasGenerationBatches(true, true)).toBe(true);
  });
});
