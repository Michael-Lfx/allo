/** model.ts 纯决策函数的单测：stepper 构造/门禁、节状态、掌握度、前置
 * 锁定、生成阶段折叠。零渲染——这正是决策逻辑抽离的目的。 */
import { describe, expect, test } from 'bun:test';
import {
  allConceptsMastered,
  buildSteps,
  canGoToStep,
  deriveStep,
  lessonStatusTagColors,
  lessonProgressLine,
  prereqLockedIds,
  sectionStatusTagColors,
  stepActivities,
  stepLocked,
} from './model';
import type { Activity, Section } from './types';
import type { Translate } from './utils';

const t = ((key: string, vars?: Record<string, unknown>) =>
  vars && Object.keys(vars).length > 0 ? `${key}:${JSON.stringify(vars)}` : key) as unknown as Translate;

const section = (section_key: string, kind: Section['kind']): Section => ({
  section_key,
  kind,
  title: `节 ${section_key}`,
  points: '',
  body_md: '',
  status: 'ready',
  version: 1,
  position: 0,
});

const activity = (id: string, section_key: string | null): Activity =>
  ({
    id,
    kind: 'single_choice',
    prompt: `q-${id}`,
    options: ['a', 'b'],
    answer: 'a',
    section_key,
  }) as unknown as Activity;

const answered = (ids: string[]) => (id: string) => ids.includes(id);

describe('buildSteps', () => {
  test('练习节收尾的新规范不含综合步', () => {
    const steps = buildSteps(
      [section('s1', 'concept'), section('s2', 'practice')],
      [activity('a1', 's2')]
    );
    expect(steps.map((step) => step.section?.section_key ?? 'general')).toEqual(['s1', 's2']);
  });

  test('旧清单（无练习节）且有通用题时追加综合步', () => {
    const steps = buildSteps([section('s1', 'concept')], [activity('a1', null)]);
    expect(steps.map((step) => step.section?.section_key ?? 'general')).toEqual(['s1', 'general']);
  });

  test('无通用题时不追加综合步', () => {
    const steps = buildSteps([section('s1', 'concept')], [activity('a1', 's1')]);
    expect(steps).toHaveLength(1);
  });
});

describe('stepActivities / 门禁', () => {
  const steps = buildSteps(
    [section('s1', 'concept'), section('s2', 'practice')],
    [activity('a1', 's2'), activity('a2', null)]
  );

  test('收尾练习轮 = 本节绑定 + 通用', () => {
    const activities = [activity('a1', 's2'), activity('a2', null)];
    expect(stepActivities(steps, 1, activities).map((a) => a.id)).toEqual(['a1', 'a2']);
  });

  test('未答完练习题禁止越过该步，答完放行', () => {
    const withTail = buildSteps(
      [section('s1', 'concept'), section('s2', 'practice'), section('s3', 'summary')],
      [activity('a1', 's2')]
    );
    expect(stepLocked(withTail, 1, [activity('a1', 's2')], answered([]))).toBe(true);
    expect(canGoToStep(withTail, 2, [activity('a1', 's2')], answered([]))).toBe(false);
    expect(canGoToStep(withTail, 2, [activity('a1', 's2')], answered(['a1']))).toBe(true);
    // 进入练习步本身不受限（门只拦越过）
    expect(canGoToStep(withTail, 1, [activity('a1', 's2')], answered([]))).toBe(true);
  });

  test('内容步与综合步不设门；随时可回退（进入目标步不含其自身门）', () => {
    const legacy = buildSteps(
      [section('s1', 'concept')],
      [activity('a1', null)]
    );
    expect(stepLocked(legacy, 0, [activity('a1', null)], answered([]))).toBe(false);
    expect(canGoToStep(legacy, 1, [activity('a1', null)], answered([]))).toBe(true);
  });
});

describe('掌握度与状态映射', () => {
  test('allConceptsMastered：空课程不算掌握，任一未达标即未全掌握', () => {
    expect(allConceptsMastered([])).toBe(false);
    expect(allConceptsMastered([{ mastery: null }, { mastery: 1 }])).toBe(false);
    expect(allConceptsMastered([{ mastery: 0.8 }, { mastery: 1 }])).toBe(true);
    expect(allConceptsMastered([{ mastery: 0.79 }])).toBe(false);
  });

  test('状态调色板覆盖全部状态键', () => {
    for (const status of ['pending', 'ready', 'failed'] as const) {
      expect(typeof sectionStatusTagColors[status]).toBe('string');
    }
    for (const status of ['not_started', 'in_progress', 'completed', 'skipped'] as const) {
      expect(typeof lessonStatusTagColors[status]).toBe('string');
    }
  });
});

describe('prereqLockedIds', () => {
  test('未完成前置锁定后继；完成/跳过前置放行', () => {
    const nodes = [
      { lesson_id: 'a', status: 'completed' as const },
      { lesson_id: 'b', status: 'in_progress' as const },
      { lesson_id: 'c', status: 'skipped' as const },
    ];
    const edges = [
      { from: 'a', to: 'b' },
      { from: 'b', to: 'c' },
    ];
    const locked = prereqLockedIds(nodes, edges);
    expect(locked.has('b')).toBe(false); // a 已完成
    expect(locked.has('c')).toBe(true); // b 未完成
  });
});

describe('deriveStep / lessonProgressLine', () => {
  test('事件流折叠：构建 → 审计 → 修复 → 发布', () => {
    const events = [
      { phase: 'round', loop: 'generate' },
      { phase: 'audit', danger: 2 },
      { phase: 'round', loop: 'repair' },
      { phase: 'publishing' },
    ] as unknown as Parameters<typeof deriveStep>[0];
    const { step, repairRounds, sawDanger } = deriveStep(events);
    expect(step).toBe(4);
    expect(repairRounds).toBe(1);
    expect(sawDanger).toBe(true);
  });

  test('空事件流退化为构建中（spinner 档）', () => {
    expect(deriveStep([]).step).toBe(1);
  });

  test('行内进度：round/audit 出文案，终态清空', () => {
    expect(lessonProgressLine({ phase: 'round', round: 3 } as never, t)).toContain(
      'learning.lessonGenRound'
    );
    expect(lessonProgressLine({ phase: 'audit', danger: 1 } as never, t)).toContain(
      'learning.lessonGenAudit'
    );
    expect(lessonProgressLine({ phase: 'completed' } as never, t)).toBeNull();
    expect(lessonProgressLine({ phase: 'failed' } as never, t)).toBeNull();
  });
});
