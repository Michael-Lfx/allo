import { describe, expect, test } from 'bun:test';
import { mergeRecentNavItems, toUpdatedAtMs } from './recentCreations';

describe('recentCreations', () => {
  test('toUpdatedAtMs scales unix seconds and keeps millis', () => {
    expect(toUpdatedAtMs(1_700_000_000)).toBe(1_700_000_000_000);
    expect(toUpdatedAtMs(1_700_000_000_000)).toBe(1_700_000_000_000);
    expect(toUpdatedAtMs('2024-01-02T00:00:00.000Z')).toBe(Date.parse('2024-01-02T00:00:00.000Z'));
  });

  test('merge includes canvas projects alongside sessions and clips', () => {
    const merged = mergeRecentNavItems(
      [{ id: 'sess-1', title: '短剧', at: 30, source: 'session' }],
      [{ id: 'clip-1', title: '直出', at: 20, source: 'task' }],
      [{ id: 'sess-1', title: '短剧成片', status: 'succeeded', updated_at: 100 }],
      [{ task_id: 'clip-1', prompt: '直出视频', status: 'succeeded', updated_at: 90 }],
      [
        { project_id: 'canvas-1', title: '我的画布', updated_at: 1_700_000_000_000 },
        { project_id: 'canvas-2', title: '更早的画布', updated_at: 1_690_000_000_000 },
      ],
      [{ id: 'brief-1', title: '播报', status: 'succeeded', updated_at: '2020-01-01T00:00:00.000Z' }],
      4
    );
    expect(merged.map((row) => row.id)).toEqual(['sess-1', 'clip-1', 'canvas-1', 'canvas-2']);
    expect(merged.find((row) => row.id === 'canvas-1')?.source).toBe('canvas');
  });

  test('server fill is recency across kinds so canvas is not buried', () => {
    const merged = mergeRecentNavItems(
      [],
      [],
      [
        { id: 'sess-old', title: '旧短剧', updated_at: 10 },
        { id: 'sess-older', title: '更旧', updated_at: 5 },
      ],
      [],
      [{ project_id: 'canvas-new', title: '新画布', updated_at: 999 }],
      [],
      2
    );
    expect(merged.map((row) => ({ id: row.id, source: row.source }))).toEqual([
      { id: 'canvas-new', source: 'canvas' },
      { id: 'sess-old', source: 'session' },
    ]);
  });

  test('keeps locally remembered canvas when the canvas list failed', () => {
    const merged = mergeRecentNavItems(
      [{ id: 'canvas-local', title: '离线画布', at: 50, source: 'canvas' }],
      [],
      [{ id: 'sess-1', title: '短剧', updated_at: 40 }],
      [],
      [],
      [],
      3,
      new Set(['session', 'task', 'briefing'])
    );
    expect(merged.map((row) => ({ id: row.id, source: row.source }))).toEqual([
      { id: 'canvas-local', source: 'canvas' },
      { id: 'sess-1', source: 'session' },
    ]);
  });
});
