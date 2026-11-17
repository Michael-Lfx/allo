/** model.ts 纯决策函数的单测：stepper 构造/门禁、节状态、生成阶段折叠。
 * 零渲染——这正是决策逻辑抽离的目的。 */
import { describe, expect, test } from 'bun:test';
import {
  buildSteps,
  coverageTopicTotals,
  deriveModernStep,
  isModernPipeline,
  progressDenominator,
  progressPercent,
  sourceCoverageState,
  triageCountsLine,
  triageDetailCountsLine,
  triageViewCounts,
  uncertainTooMany,
  canGoToStep,
  deriveStep,
  lessonStatusTagColors,
  lessonProgressLine,
  progressReadoutText,
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

  test('行内进度：裸 round 出通用「生成中」不带轮次数字，audit 按环区分，终态清空', () => {
    // f1：裸轮次数字退出学习者面——round 帧只出通用词条（向后兼容兜底）。
    const bareRound = lessonProgressLine({ phase: PHASE.round, round: 3 } as never, t);
    expect(bareRound).toContain('learning.lessonGenRunning');
    expect(bareRound).not.toContain('learning.lessonGenRound');
    expect(bareRound).toBe('learning.lessonGenRunning');
    // 生成环审计：既有质量检查词条。
    expect(
      lessonProgressLine({ phase: PHASE.audit, loop: PHASE.generate, danger: 1 } as never, t)
    ).toContain('learning.lessonGenAudit');
    // 修复环审计：独立「修复审校」词条（不再与生成环计数混淆）。
    expect(
      lessonProgressLine({ phase: PHASE.audit, loop: PHASE.repair, danger: 2 } as never, t)
    ).toContain('learning.lessonGenAuditRepair');
    // 终态清空；发布相位不再清空（出「发布中」——M1 相位链补全）。
    expect(lessonProgressLine({ phase: PHASE.completed } as never, t)).toBeNull();
    expect(lessonProgressLine({ phase: PHASE.failed } as never, t)).toBeNull();
    expect(lessonProgressLine({ phase: PHASE.publishing } as never, t)).toBe(
      'learning.lessonGenPublishing'
    );
    // 出题相位（ls_patch_activities 成功路径帧）。
    expect(lessonProgressLine({ phase: PHASE.activities } as never, t)).toBe(
      'learning.lessonGenActivities'
    );
  });

  test('f1 相位帧：plan 出「规划节清单」，section 出「写第 k/N 节」带耗时', () => {
    // plan 帧：N 不上学习者面（节数在 stepper 面自现），只报相位。
    const plan = lessonProgressLine({ phase: PHASE.plan, total: 5 } as never, t);
    expect(plan).toBe('learning.lessonGenPlan');
    // section 帧 + 耗时：写第 k/N 节 · 已 elapsed（共享耗时后缀拼装）。
    expect(
      lessonProgressLine(
        { phase: PHASE.section, index: 2, total: 5, title: '概念：期权的定义' } as never,
        t,
        '1:00'
      )
    ).toBe(
      'learning.lessonGenSection:' +
        JSON.stringify({ index: 2, total: 5 }) +
        ' · learning.lessonGenElapsed:' +
        JSON.stringify({ elapsed: '1:00' })
    );
    // 耗时缺席（started 帧丢失/订阅中途）：退无耗时词条，不渲染空「已 」。
    expect(
      lessonProgressLine({ phase: PHASE.section, index: 3, total: 5 } as never, t)
    ).toBe('learning.lessonGenSection:' + JSON.stringify({ index: 3, total: 5 }));
  });

  test('M2 耗时全程挂：plan/activities/audit/修复审校/publishing 全带耗时，round 兜底不带', () => {
    // 耗时后缀统一「 · 已 elapsed」，与 section 帧同一份拼装。
    expect(lessonProgressLine({ phase: PHASE.plan } as never, t, '0:30')).toBe(
      'learning.lessonGenPlan · learning.lessonGenElapsed:' + JSON.stringify({ elapsed: '0:30' })
    );
    expect(lessonProgressLine({ phase: PHASE.activities } as never, t, '2:00')).toBe(
      'learning.lessonGenActivities · learning.lessonGenElapsed:' +
        JSON.stringify({ elapsed: '2:00' })
    );
    expect(lessonProgressLine({ phase: PHASE.publishing } as never, t, '3:00')).toBe(
      'learning.lessonGenPublishing · learning.lessonGenElapsed:' +
        JSON.stringify({ elapsed: '3:00' })
    );
    expect(
      lessonProgressLine({ phase: PHASE.audit, loop: PHASE.generate, danger: 1 } as never, t, '4:00')
    ).toBe(
      'learning.lessonGenRunning · learning.lessonGenAudit:' +
        JSON.stringify({ danger: 1, warning: 0 }) +
        ' · learning.lessonGenElapsed:' +
        JSON.stringify({ elapsed: '4:00' })
    );
    expect(
      lessonProgressLine({ phase: PHASE.audit, loop: PHASE.repair, danger: 0 } as never, t, '5:00')
    ).toBe(
      'learning.lessonGenAuditRepair:' +
        JSON.stringify({ danger: 0, warning: 0 }) +
        ' · learning.lessonGenElapsed:' +
        JSON.stringify({ elapsed: '5:00' })
    );
    // 裸 round 兜底（向后兼容）不带耗时——保持既有语义。
    expect(lessonProgressLine({ phase: PHASE.round } as never, t, '6:00')).toBe(
      'learning.lessonGenRunning'
    );
    // 耗时缺席：全部退无耗时词条（降级路径语义不变）。
    expect(lessonProgressLine({ phase: PHASE.plan } as never, t)).toBe('learning.lessonGenPlan');
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

  test('明细首行（F4）：混合/重复/不确定三个残余计数收进 triage 明细折叠', () => {
    // 三个计数互不相同：交错接线（mixed↔uncertain 换位等）当场红。
    const line = triageDetailCountsLine({ ...counts, mixed: 3, duplicate: 2, uncertain: 5 }, t);
    expect(line).toContain('learning.genTriageDetailCounts');
    expect(line).toContain('"mixed":3');
    expect(line).toContain('"duplicate":2');
    expect(line).toContain('"uncertain":5');
  });

  test('明细首行（F4）：三个计数全为 0 时不出行（不渲染「0、0、0」噪声行）', () => {
    expect(triageDetailCountsLine({ ...counts, mixed: 0, duplicate: 0, uncertain: 0 }, t)).toBeNull();
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

// ── f2：课程进度分母（剔除 skipped）────────────────────────────────────

describe('f2 进度分母与百分比', () => {
  test('分母 = total − skipped：无跳过时与旧口径一致，跳过逐课退出分母', () => {
    expect(progressDenominator({ total_lessons: 19, skipped_lessons: 0 })).toBe(19);
    expect(progressDenominator({ total_lessons: 19, skipped_lessons: 15 })).toBe(4);
    expect(progressDenominator({ total_lessons: 3, skipped_lessons: 1 })).toBe(2);
  });

  test('全跳 → 分母 0（无可计进度，消费面呈「—」不渲染进度条）', () => {
    expect(progressDenominator({ total_lessons: 3, skipped_lessons: 3 })).toBe(0);
    expect(progressDenominator({ total_lessons: 0, skipped_lessons: 0 })).toBe(0);
  });

  test('脏数据钳制：skipped > total 分母不为负（钳 ≥0）', () => {
    expect(progressDenominator({ total_lessons: 3, skipped_lessons: 5 })).toBe(0);
  });

  test('percent：completed ÷ 分母四舍五入；分母 0 → null（「—」分支）', () => {
    expect(progressPercent(2, 4)).toBe(50);
    expect(progressPercent(1, 3)).toBe(33);
    expect(progressPercent(3, 3)).toBe(100);
    expect(progressPercent(0, 0)).toBeNull();
    expect(progressPercent(3, 0)).toBeNull();
  });
});

describe('S3 进度读数文本（共享展示面）', () => {
  test('有分母出 a/N，分母 0 出「—」；尾缀只在 a/N 臂拼（「—」臂不带）', () => {
    expect(progressReadoutText(2, 4)).toBe('2/4');
    expect(progressReadoutText(0, 4)).toBe('0/4');
    expect(progressReadoutText(1, 0)).toBe('—');
    // CourseCard 形：a/N 后随「 N 课时」词条；分母 0 臂只有「—」不带尾缀。
    expect(progressReadoutText(2, 4, ' 4 课时')).toBe('2/4 4 课时');
    expect(progressReadoutText(2, 0, ' 4 课时')).toBe('—');
  });
});
