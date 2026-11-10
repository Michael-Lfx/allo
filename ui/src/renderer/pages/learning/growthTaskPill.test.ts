/** growthTaskPill.ts 纯状态面的单测（g4 生长全局悬浮 Pill）：轮询快照为准
 * （入列/终态/消失都只听 HTTP）、WS 事件仅增强（打回计数，不判终态）。
 * 零渲染——状态纪律抽离正是为了这一层可测。 */
import { describe, expect, test } from 'bun:test';
import {
  applyGrowthEvent,
  dismissGrowthItem,
  GROWTH_POLL_ACTIVE_MS,
  GROWTH_POLL_IDLE_MS,
  growthPollDelay,
  mergeGrowthSnapshot,
} from './growthTaskPill';
import type { LearningGenerationTaskView } from './types';

const row = (overrides: Partial<LearningGenerationTaskView> = {}): LearningGenerationTaskView => ({
  key: 'growth:course-a:-',
  kind: 'growth',
  course_id: 'course-a',
  node_id: null,
  topic: '学习图生长 · 线性代数',
  state: 'running',
  step: 'draft',
  detail: '教练正在阅读结构参考与花名册，起草本批节点…',
  elapsed_secs: 12,
  queued_at_ms: null,
  queue_position: null,
  ...overrides,
});

describe('mergeGrowthSnapshot（轮询为准）', () => {
  test('在跑/排队的 growth 行入列；其它任务类别与未跟踪过的终态行不入列', () => {
    const items = mergeGrowthSnapshot(
      [],
      [
        row(),
        row({
          key: 'growth:course-b:-',
          course_id: 'course-b',
          state: 'queued',
          elapsed_secs: null,
          queue_position: 1,
        }),
        row({ key: 'lesson:course-a:node-1', kind: 'lesson', node_id: 'node-1' }),
        row({ key: 'growth:course-c:-', course_id: 'course-c', state: 'done' }),
      ]
    );
    expect(items.map((item) => [item.courseId, item.status])).toEqual([
      ['course-a', 'running'],
      ['course-b', 'queued'],
    ]);
    expect(items[1].queuePosition).toBe(1);
  });

  test('已跟踪项按快照刷新（进度与终态都以 HTTP 为准），同键不重复入列', () => {
    const running = mergeGrowthSnapshot([], [row()]);
    const progressed = mergeGrowthSnapshot(running, [row({ elapsed_secs: 40 })]);
    expect(progressed.map((item) => [item.status, item.elapsedSecs])).toEqual([['running', 40]]);

    const failed = mergeGrowthSnapshot(progressed, [
      row({ state: 'failed', step: null, detail: '模型调用超时', elapsed_secs: null }),
    ]);
    expect(failed).toHaveLength(1);
    expect(failed[0].status).toBe('failed');
    expect(failed[0].detail).toBe('模型调用超时');
  });

  test('快照里已没有的跟踪项出列（不臆造终态）', () => {
    const running = mergeGrowthSnapshot([], [row()]);
    expect(mergeGrowthSnapshot(running, [])).toEqual([]);
  });
});

describe('applyGrowthEvent（WS 仅增强）', () => {
  test('WS 终态帧不判终态：仍以运行中呈现，直到快照给出终态', () => {
    const running = mergeGrowthSnapshot([], [row()]);
    const afterFrame = applyGrowthEvent(running, {
      course_id: 'course-a',
      event: 'growth_completed',
    });
    expect(afterFrame[0].status).toBe('running');
    expect(mergeGrowthSnapshot(afterFrame, [row({ state: 'done' })])[0].status).toBe('done');
  });

  test('打回帧按课程累计重起草次数；未跟踪课程的帧不入列', () => {
    let items = mergeGrowthSnapshot([], [row()]);
    items = applyGrowthEvent(items, { course_id: 'course-a', event: 'growth_redraft', stage: 'gates' });
    items = applyGrowthEvent(items, {
      course_id: 'course-a',
      event: 'growth_redraft',
      stage: 'concept_review',
    });
    items = applyGrowthEvent(items, { course_id: 'course-z', event: 'growth_redraft', stage: 'gates' });
    expect(items).toHaveLength(1);
    expect(items[0].redraft).toEqual({ stage: 'concept_review', count: 2 });
    // 后续快照（仍在跑）保留增强读数；到终态即清。
    items = mergeGrowthSnapshot(items, [row({ elapsed_secs: 90 })]);
    expect(items[0].redraft).toEqual({ stage: 'concept_review', count: 2 });
    items = mergeGrowthSnapshot(items, [row({ state: 'done' })]);
    expect(items[0].redraft).toBeNull();
  });
});

describe('dismissGrowthItem / growthPollDelay', () => {
  test('只有终态项可被关闭；关闭后同一终态快照不再复活', () => {
    const running = mergeGrowthSnapshot([], [row()]);
    expect(dismissGrowthItem(running, 'growth:course-a:-')).toHaveLength(1);

    const done = mergeGrowthSnapshot(running, [row({ state: 'done' })]);
    const dismissed = dismissGrowthItem(done, 'growth:course-a:-');
    expect(dismissed).toEqual([]);
    expect(mergeGrowthSnapshot(dismissed, [row({ state: 'done' })])).toEqual([]);
  });

  test('有活跃项走快轮询，否则慢轮询兜底（WS 缺席也能发现新任务）', () => {
    const running = mergeGrowthSnapshot([], [row()]);
    const done = mergeGrowthSnapshot(running, [row({ state: 'done' })]);
    expect(growthPollDelay(running)).toBe(GROWTH_POLL_ACTIVE_MS);
    expect(growthPollDelay(done)).toBe(GROWTH_POLL_IDLE_MS);
    expect(growthPollDelay([])).toBe(GROWTH_POLL_IDLE_MS);
    expect(GROWTH_POLL_ACTIVE_MS).toBeLessThan(GROWTH_POLL_IDLE_MS);
  });
});
