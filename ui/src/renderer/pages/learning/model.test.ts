/** model.ts 纯决策函数的单测：stepper 构造/门禁、节状态、生成阶段折叠。
 * 零渲染——这正是决策逻辑抽离的目的。 */
import { describe, expect, test } from 'bun:test';
import {
  buildSteps,
  coverageTopicTotals,
  deriveModernStep,
  isModernPipeline,
  sourceCoverageState,
  triageCountsLine,
  triageViewCounts,
  uncertainTooMany,
  canGoToStep,
  deriveStep,
  lessonStatusTagColors,
  lessonProgressLine,
  sectionStatusTagColors,
  stepActivities,
  stepLocked,
} from './model';
import type { Activity, Section } from './types';
import { PHASE } from './phaseMeta';
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

describe('状态映射', () => {
  test('状态调色板覆盖全部状态键', () => {
    for (const status of ['pending', 'ready', 'failed'] as const) {
      expect(typeof sectionStatusTagColors[status]).toBe('string');
    }
    for (const status of ['not_started', 'in_progress', 'completed', 'skipped'] as const) {
      expect(typeof lessonStatusTagColors[status]).toBe('string');
    }
  });
});

describe('deriveStep / lessonProgressLine', () => {
  test('事件流折叠：构建 → 审计 → 修复 → 发布', () => {
    const events = [
      { phase: PHASE.round, loop: PHASE.generate },
      { phase: PHASE.audit, danger: 2 },
      { phase: PHASE.round, loop: PHASE.repair },
      { phase: PHASE.publishing },
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
    expect(lessonProgressLine({ phase: PHASE.round, round: 3 } as never, t)).toContain(
      'learning.lessonGenRound'
    );
    expect(lessonProgressLine({ phase: PHASE.audit, danger: 1 } as never, t)).toContain(
      'learning.lessonGenAudit'
    );
    expect(lessonProgressLine({ phase: PHASE.completed } as never, t)).toBeNull();
    expect(lessonProgressLine({ phase: PHASE.failed } as never, t)).toBeNull();
  });
});

// ── t5：新管线（清点分诊 → 来源结构计划 → 分批展开）的纯决策面 ──────────

describe('t5 新管线步骤折叠', () => {
  test('triage/source_plan/coverage 阶段推进到对应档', () => {
    const base = { phase: PHASE.triageStarted } as never;
    expect(deriveModernStep([base])).toBe(1);
    expect(
      deriveModernStep([{ phase: PHASE.triageCompleted } as never])
    ).toBe(1);
    expect(
      deriveModernStep([
        { phase: PHASE.triageCompleted } as never,
        { phase: PHASE.sourcePlanStarted } as never,
      ])
    ).toBe(2);
    expect(
      deriveModernStep([
        { phase: PHASE.sourcePlanReady, chapters: 3 } as never,
      ])
    ).toBe(2);
    expect(
      deriveModernStep([{ phase: PHASE.coverageReady } as never])
    ).toBe(3);
    expect(
      deriveModernStep([
        { phase: PHASE.coverageReady } as never,
        { phase: PHASE.publishing } as never,
      ])
    ).toBe(3);
    expect(
      deriveModernStep([
        { phase: PHASE.coverageReady } as never,
        { phase: PHASE.completed } as never,
      ])
    ).toBe(4);
  });

  test('空流退化到分诊中档（spinner 不挤掉等待提示）', () => {
    expect(deriveModernStep([])).toBe(1);
  });

  test('新管线判定：看到 triage 阶段即切换新步骤条', () => {
    expect(isModernPipeline([{ phase: PHASE.triageStarted } as never])).toBe(true);
    expect(isModernPipeline([{ phase: PHASE.triageCompleted } as never])).toBe(true);
    expect(
      isModernPipeline([
        { phase: PHASE.started } as never,
        { phase: PHASE.round, loop: PHASE.generate } as never,
      ])
    ).toBe(false);
    expect(isModernPipeline([])).toBe(false);
  });

  test('声明字段优先于 triage 嗅探（x11）', () => {
    expect(isModernPipeline([], 'source_plan')).toBe(true);
    expect(
      isModernPipeline([{ phase: PHASE.triageStarted } as never], 'sampling')
    ).toBe(false);
    // 无声明时退回嗅探（滚动部署期旧帧兜底）。
    expect(isModernPipeline([{ phase: PHASE.triageStarted } as never])).toBe(true);
  });
});

describe('t5 uncertain 过多判定与分诊计数适配', () => {
  const counts = {
    total: 8,
    core: 2,
    prerequisite: 1,
    practice: 1,
    mixed: 1,
    noise: 1,
    duplicate: 1,
    uncertain: 1,
  };

  test('扫描行：总数 + 核心/跳过读数（混合/重复/不确定进明细，不上主行）', () => {
    const line = triageCountsLine(counts, t);
    expect(line).toContain('learning.genTriageLine');
    expect(line).toContain('"total":8');
    expect(line).toContain('"core":2');
    expect(line).toContain('"skipped":2');
    expect(line).not.toContain('"mixed"');
    expect(line).not.toContain('"duplicate"');
    expect(line).not.toContain('"uncertain"');
  });

  test('uncertain 过多：数量 ≥10 或超过 core 时提醒整理知识库', () => {
    expect(uncertainTooMany({ ...counts, uncertain: 10 })).toBe(true);
    expect(uncertainTooMany({ ...counts, core: 1, uncertain: 5 })).toBe(true);
    expect(uncertainTooMany(counts)).toBe(false);
    expect(uncertainTooMany({ ...counts, uncertain: 9, core: 9 })).toBe(false);
  });

  test('DTO 分诊摘要（total_files 字段面）适配成计数入参', () => {
    expect(triageViewCounts({
      total_files: 8,
      core: 2,
      prerequisite: 1,
      practice: 1,
      mixed: 1,
      noise: 1,
      duplicate: 1,
      uncertain: 1,
    })).toEqual(counts);
  });
});

// 计数行/成本门读数行（triageCountsLine / estimateLines）已随计划确认页移除（d5）。

// ── t5：课程详情「来源覆盖」卡的三态判定与 topic 总数 ──────────────────────

describe('t5 来源覆盖卡三态', () => {
  test('null（旧课程/描述流）不显示空卡；available=false 显式不可用；正常快照 ready', () => {
    expect(sourceCoverageState(null)).toBe('hidden');
    expect(
      sourceCoverageState({ snapshot: { version: null, available: false } } as never)
    ).toBe('unavailable');
    expect(
      sourceCoverageState({ snapshot: { version: 1, available: true } } as never)
    ).toBe('ready');
  });

  test('topic 总数 = covered + skipped + uncertain 标签数（四态里 uncertain 无计数面）', () => {
    expect(coverageTopicTotals({ topics_covered: 3, topics_skipped: 1 } as never)).toEqual({
      covered: 3,
      skipped: 1,
      total: 4,
    });
    expect(
      coverageTopicTotals({
        topics_covered: 3,
        topics_skipped: 1,
        uncertain: ['a', 'b'],
      } as never)
    ).toEqual({ covered: 3, skipped: 1, total: 6 });
  });
});
