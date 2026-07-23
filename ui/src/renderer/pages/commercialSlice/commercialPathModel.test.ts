

import { describe, expect, test } from 'bun:test';
import { COMMERCIAL_PATH_FRAMES } from './commercialPathModel';

describe('commercial path prototype', () => {
  test('covers launchpad readiness states including missing workspace', () => {
    expect(COMMERCIAL_PATH_FRAMES.map((frame) => frame.state).sort()).toEqual(
      [
        'first_user',
        'missing_model',
        'missing_workspace',
        'model_failure',
        'network_failure',
        'returning_user',
        'task_success',
      ].sort()
    );
    expect(new Set(COMMERCIAL_PATH_FRAMES.map((frame) => frame.scene)).size).toBeGreaterThanOrEqual(3);
  });

  test('ready / missing_model / missing_workspace expose status chips and plan preview', () => {
    for (const state of ['returning_user', 'missing_model', 'missing_workspace'] as const) {
      const frame = COMMERCIAL_PATH_FRAMES.find((item) => item.state === state);
      expect(frame?.statusChips?.length).toBeGreaterThanOrEqual(3);
      expect(frame?.planPreview).toBeTruthy();
    }
  });
});
