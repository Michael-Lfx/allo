/** systemActions.ts 纯投影函数的单测（y04）：动作行 → 展示模型（类型/触发源/
 * 结果徽标/失败 alert/reason）+ 批行 cancelled 中性 tag。零渲染——把决策
 * 逻辑抽离正是为了这一层可测（autoRefill.test.ts 同款纪律）。 */
import { describe, expect, test } from 'bun:test';
import {
  batchStatusMeta,
  buildSystemActionRow,
  buildSystemActionRows,
  typeLabelKeys,
} from './systemActions';
import type { SystemActionView } from './types';

const action = (overrides: Partial<SystemActionView> = {}): SystemActionView => ({
  action_id: '0190f5fe-7c00-7a00-8abc-0123456789aa',
  kind: 'maintenance_repair',
  trigger: 'timer',
  basis: 'unconsumed=2 observations=0',
  result: 'done',
  reason: '',
  actor_user_id: null,
  at: 2000,
  ...overrides,
});

describe('buildSystemActionRow', () => {
  test('done 行：绿色完成徽标、无 alert、无 reason', () => {
    const row = buildSystemActionRow(action());
    expect(row.resultColor).toBe('green');
    expect(row.resultRole).toBeUndefined();
    expect(row.reason).toBe('');
    expect(row.basis).toBe('unconsumed=2 observations=0');
    expect(row.kindKey).toBe(typeLabelKeys.kind.maintenance_repair);
    expect(row.triggerKey).toBe(typeLabelKeys.trigger.timer);
  });

  test('failed 行：红色失败徽标 + role=alert + reason 如实', () => {
    const row = buildSystemActionRow(
      action({ result: 'failed', reason: 'model unavailable' })
    );
    expect(row.resultColor).toBe('red');
    expect(row.resultRole).toBe('alert');
    expect(row.reason).toBe('model unavailable');
    expect(row.resultKey).toBe(typeLabelKeys.result.failed);
  });

  test('未知 kind/trigger/result 走宽容兜底 key（读侧不落错）', () => {
    const row = buildSystemActionRow(
      action({ kind: 'future_kind', trigger: 'future_trigger', result: 'future_result' })
    );
    expect(row.kindKey).toBe(typeLabelKeys.kind.unknown);
    expect(row.triggerKey).toBe(typeLabelKeys.trigger.unknown);
    expect(row.resultKey).toBe(typeLabelKeys.result.unknown);
    expect(row.resultColor).toBe('gray');
  });
});

describe('buildSystemActionRows', () => {
  test('空列表 = 空态（课程从未有系统动作）', () => {
    const model = buildSystemActionRows([]);
    expect(model.empty).toBe(true);
    expect(model.rows).toHaveLength(0);
  });

  test('非空列表逐行投影，保留后端倒序', () => {
    const model = buildSystemActionRows([
      action({ action_id: 'a', at: 3000 }),
      action({ action_id: 'b', at: 2000 }),
    ]);
    expect(model.empty).toBe(false);
    expect(model.rows.map((row) => row.key)).toEqual(['a', 'b']);
    expect(model.rows.map((row) => row.at)).toEqual([3000, 2000]);
  });
});

describe('batchStatusMeta', () => {
  test('cancelled = 中性灰色 tag（不是失败红）', () => {
    const meta = batchStatusMeta('cancelled');
    expect(meta).toBeDefined();
    expect(meta?.neutral).toBe(true);
    expect(meta?.color).toBe('gray');
    expect(meta?.labelKey).toBe('learning.learningGraphBatchCancelled');
  });

  test('failed 仍是红色失败 tag', () => {
    const meta = batchStatusMeta('failed');
    expect(meta?.neutral).toBe(false);
    expect(meta?.color).toBe('red');
  });

  test('applied/pending 不上状态 tag（applied 无 tag；pending 走 processing）', () => {
    expect(batchStatusMeta('applied')).toBeUndefined();
    expect(batchStatusMeta('pending')?.color).toBe('processing');
  });
});
