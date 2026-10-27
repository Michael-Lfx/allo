/** autoRefill.ts 纯决策函数的单测（m10 触发②前端臂）：自动触发判定 +
 * 会话重入集。零渲染——决策逻辑抽离正是为了这一层可测。 */
import { describe, expect, test } from 'bun:test';
import { AutoRefillSessionGate, shouldAutoFireGrowth } from './autoRefill';
import type { GrowthVerdictView } from './types';

const view = (overrides: Partial<GrowthVerdictView> = {}): GrowthVerdictView => ({
  verdict: 'due',
  ready_count: 2,
  trigger: 3,
  enabled: true,
  stalled: false,
  ...overrides,
});

describe('shouldAutoFireGrowth', () => {
  test('due 且非停摆 = 自动补批', () => {
    expect(shouldAutoFireGrowth(view())).toBe(true);
  });

  test('due 但停摆臂（空图）= 抑制（ADR-0010 只留空图臂）', () => {
    expect(shouldAutoFireGrowth(view({ stalled: true }))).toBe(false);
  });

  test('below_threshold（水位以上）= 不动作', () => {
    expect(shouldAutoFireGrowth(view({ verdict: 'below_threshold', ready_count: 3 }))).toBe(false);
  });

  test('disabled（开关合力关）= 如实不动作', () => {
    expect(
      shouldAutoFireGrowth(view({ verdict: 'disabled', enabled: false, ready_count: 0 }))
    ).toBe(false);
  });
});

describe('AutoRefillSessionGate（learnhub BTreeSet 语义域中立化）', () => {
  test('每课每会话至多标记一次，同课二次被拦', () => {
    const gate = new AutoRefillSessionGate();
    expect(gate.tryMark('course-a')).toBe(true);
    expect(gate.tryMark('course-a')).toBe(false);
    expect(gate.tryMark('course-b')).toBe(true);
  });

  test('release 后同课可重试（派发失败不挂死配额）', () => {
    const gate = new AutoRefillSessionGate();
    expect(gate.tryMark('course-a')).toBe(true);
    gate.release('course-a');
    expect(gate.tryMark('course-a')).toBe(true);
  });

  test('release 未标记的课不报错', () => {
    const gate = new AutoRefillSessionGate();
    gate.release('course-never-marked');
    expect(gate.tryMark('course-never-marked')).toBe(true);
  });
});
