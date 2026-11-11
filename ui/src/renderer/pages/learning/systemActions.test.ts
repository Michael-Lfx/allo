/** systemActions.ts 纯投影函数的单测（y04）：动作行 → 展示模型（类型/触发源/
 * 结果徽标/失败 alert/reason）+ 批行 cancelled 中性 tag。零渲染——把决策
 * 逻辑抽离正是为了这一层可测（autoRefill.test.ts 同款纪律）。 */
import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
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
    expect(row.reasonKey).toBe('');
    expect(row.basis).toBe('unconsumed=2 observations=0');
    expect(row.kindKey).toBe(typeLabelKeys.kind.maintenance_repair);
    expect(row.triggerKey).toBe(typeLabelKeys.trigger.timer);
  });

  test('failed 行：红色失败徽标 + role=alert + reason 走分类映射 key（原文不直出）', () => {
    const row = buildSystemActionRow(
      action({ result: 'failed', reason: 'Request timeout: json coach call exceeded 480s' })
    );
    expect(row.resultColor).toBe('red');
    expect(row.resultRole).toBe('alert');
    expect(row.reasonKey).toBe('learning.systemActionReasonTimeout');
    expect(row.reasonKey).not.toContain('480s');
    expect(row.resultKey).toBe(typeLabelKeys.result.failed);
  });

  test('failed 行 reason 分类：冲突/产物未过校验/服务繁忙/未知原因兜底', () => {
    const cases: ReadonlyArray<[string, string]> = [
      [
        'Conflict: course generation for this source is already running',
        'learning.systemActionReasonConflict',
      ],
      ['Unprocessable entity: - [gd.terrain.keys] 未知顶层键「type」', 'learning.systemActionReasonInvalidOutput'],
      ['Rate limited', 'learning.systemActionReasonBusy'],
      ['Bad gateway: upstream closed', 'learning.systemActionReasonBusy'],
      ['something entirely new', 'learning.systemActionReasonUnknown'],
      ['   ', 'learning.systemActionReasonUnknown'],
    ];
    for (const [reason, expected] of cases) {
      const row = buildSystemActionRow(action({ result: 'failed', reason }));
      expect(row.reasonKey).toBe(expected);
    }
  });

  test('reason 分类语义（F6）：not implemented = 能力未落地，归中性「暂不支持」而非「产物未过校验」', () => {
    const row = buildSystemActionRow(
      action({ result: 'failed', reason: 'Not implemented: structure repair v2' })
    );
    expect(row.reasonKey).toBe('learning.systemActionReasonUnsupported');
  });

  test('reason 分类语义（F6）：internal error = 服务端故障，归「服务暂时不可用」而非「繁忙」；Bad gateway 顶层类别优先', () => {
    const cases: ReadonlyArray<[string, string]> = [
      ['Internal error: db pool exhausted', 'learning.systemActionReasonServiceUnavailable'],
      // 顶层类别优先：Bad gateway 包着下游 internal error 仍归繁忙档（AppError
      // Display 串以最外层变体开头）。
      ['Bad gateway: Agent internal error (code -32603)', 'learning.systemActionReasonBusy'],
      ['No usable model provider is configured: all providers disabled', 'learning.systemActionReasonBusy'],
      ['Unprocessable entity: - [gd.terrain.keys] 未知顶层键「type」', 'learning.systemActionReasonInvalidOutput'],
      ['Conflict: course generation for this source is already running', 'learning.systemActionReasonConflict'],
      ['Request timeout: json coach call exceeded 480s', 'learning.systemActionReasonTimeout'],
    ];
    for (const [reason, expected] of cases) {
      expect(buildSystemActionRow(action({ result: 'failed', reason })).reasonKey).toBe(expected);
    }
  });

  test('reason 原文收纳（F5）：失败行 reasonDetail 保留诊断原文；done 行与空原文不携带', () => {
    const failed = buildSystemActionRow(
      action({ result: 'failed', reason: 'Request timeout: json coach call exceeded 480s' })
    );
    // o1 曾把原文整体丢弃；评审裁决收进 Tooltip（收纳不删信息）。
    expect(failed.reasonDetail).toBe('Request timeout: json coach call exceeded 480s');
    const done = buildSystemActionRow(action());
    expect(done.reasonDetail).toBe('');
    // 空原文：分类句照常落「未知原因」，但无可收纳的原文，不出 Tooltip。
    const blank = buildSystemActionRow(action({ result: 'failed', reason: '   ' }));
    expect(blank.reasonKey).toBe('learning.systemActionReasonUnknown');
    expect(blank.reasonDetail).toBe('');
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

describe('SystemActionsSection 接线（源面契约，plainLanguageDictionary 形态）', () => {
  // SystemActionsSection 未导出且读数在 effect 内，SSR 渲染够不到行体；
  // 接线事实按本波既有源面契约 seam 断言（F5：原文只进 Tooltip 收纳）。
  const workspaceSource = readFileSync(
    new URL('./components/LearningGraphWorkspace.tsx', import.meta.url),
    'utf8'
  );

  test('失败原因：分类句为主显示，原文经 Tooltip 收纳（GrowthTaskPill failed detail 同形状）', () => {
    // includes + toBe：失败时不把整份源码倒进报告（structureReadout 接线断言同款）。
    expect(workspaceSource.includes('<Tooltip content={row.reasonDetail}>')).toBe(true);
    // 主显示文本位是分类句，不是原文。
    expect(workspaceSource.includes('{t(row.reasonKey)}')).toBe(true);
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
