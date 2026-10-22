/**
 * 学习模块的纯决策函数层（stepper 构造、节状态语义、锁定推导、掌握度
 * 规则、生成阶段折叠）：零渲染、零副作用，全部可直接单测——组件只做组
 * 合与展示，业务决策都收在这里（避免决策逻辑埋在渲染体里无法测试）。
 */
import type { ILearningCourseGenerationEvent, ILearningGenerationToolCall } from '@/common/adapter/ipcBridge';
import type { Activity, LessonStatus, Section } from './types';
import type { Translate } from './utils';

/** 课时状态 → Arco Tag 颜色（课程大纲/工作区/学习图详情共用） */
export const lessonStatusTagColors: Record<LessonStatus, string> = {
  not_started: 'gray',
  in_progress: 'blue',
  completed: 'green',
  skipped: 'gray',
};

// ── Section 状态语义（ADR-0003 前端面）────────────────────────────────

export type SectionStatus = Section['status'];

/** 节状态 → Arco Tag 颜色 */
export const sectionStatusTagColors: Record<SectionStatus, string> = {
  pending: 'gray',
  ready: 'green',
  failed: 'red',
};

/** 节状态标签（stepper/重写入口共用；pending = 尚未生成到该节） */
export function sectionStatusLabel(status: SectionStatus, t: Translate): string {
  const labels: Record<SectionStatus, string> = {
    pending: t('learning.sectionStatusPending'),
    ready: t('learning.sectionStatusReady'),
    failed: t('learning.sectionStatusFailed'),
  };
  return labels[status];
}

// ── 分节 stepper（新规范：练习节 = 一等练习轮；旧清单回退综合步）──────

/** 分节交付的一步：节正文 + 该节绑定的练习题；null = 综合题收尾步 */
export interface LessonStep {
  section: Section | null;
}

/** 步骤数组：练习节收尾的新规范不含综合步；旧清单（无练习节）且有通用
 * 题时在末尾追加综合步，不与单节内容混排。 */
export function buildSteps(sections: Section[], activities: Activity[]): LessonStep[] {
  const hasPractice = sections.some((section) => section.kind === 'practice');
  const hasGeneral = activities.some((activity) => activity.section_key === null);
  return [
    ...sections.map((section) => ({ section })),
    ...(hasPractice || !hasGeneral ? [] : [{ section: null }]),
  ];
}

/** 某一步的作答轮：练习节出自己绑定的题；收尾练习轮（最后一个练习步）
 * 收编更早练习节未出的题 + 内容节绑定题 + 通用综合题；内容步与综合步不
 * 设门（门禁只拦练习步，见 ADR-0002 追加决策）。 */
export function stepActivities(
  steps: LessonStep[],
  index: number,
  activities: Activity[]
): Activity[] {
  const entry = steps[index];
  if (!entry?.section || entry.section.kind !== 'practice') return [];
  const lastPracticeIndex = findLastPracticeIndex(steps);
  if (index !== lastPracticeIndex) {
    return activities.filter((activity) => activity.section_key === entry.section?.section_key);
  }
  // 收尾轮：除更早练习节已出的题外全部归此（本节绑定 + 内容节绑定 + 通用）
  const earlierPracticeKeys = new Set<string>();
  steps.slice(0, lastPracticeIndex).forEach((step) => {
    if (step.section?.kind === 'practice') earlierPracticeKeys.add(step.section.section_key);
  });
  return activities.filter(
    (activity) =>
      activity.section_key === null || !earlierPracticeKeys.has(activity.section_key)
  );
}

function findLastPracticeIndex(steps: LessonStep[]): number {
  for (let index = steps.length - 1; index >= 0; index -= 1) {
    if (steps[index]?.section?.kind === 'practice') return index;
  }
  return -1;
}

/** 步进门禁：该步的题未全部作答则禁止越过（进入不受限，可随时回退） */
export function stepLocked(
  steps: LessonStep[],
  index: number,
  activities: Activity[],
  isAnswered: (activityId: string) => boolean
): boolean {
  const round = stepActivities(steps, index, activities);
  return round.length > 0 && !round.every((activity) => isAnswered(activity.id));
}

/** 目标步可达 = 途中每一道门都已过（不含目标步自身） */
export function canGoToStep(
  steps: LessonStep[],
  target: number,
  activities: Activity[],
  isAnswered: (activityId: string) => boolean
): boolean {
  for (let index = 0; index < target; index += 1) {
    if (stepLocked(steps, index, activities, isAnswered)) return false;
  }
  return true;
}

// ── 学习图前置锁定（DAG 与列表视图共用）──────────────────────────────

// ── 生成阶段折叠（WS 事件流 → 展示态）────────────────────────────────

/** 生成步骤条四段：准备 → 构建大纲 → 审计 → 修复（仅审计不过时出现）→ 完成 */
export type GenStep = 0 | 1 | 2 | 3 | 4;

/** 把已收到的事件流折叠成步骤条状态：修复轮数与是否出现过 DANGER 审计 */
export function deriveStep(events: ILearningCourseGenerationEvent[]): {
  step: GenStep;
  repairRounds: number;
  sawDanger: boolean;
} {
  let step: GenStep = 0;
  let repairRounds = 0;
  let sawDanger = false;
  for (const event of events) {
    if (event.phase === 'round' && event.loop === 'repair') {
      step = Math.max(step, 3) as GenStep;
      repairRounds += 1;
    } else if (event.phase === 'audit') {
      step = Math.max(step, 2) as GenStep;
      if ((event.danger ?? 0) > 0) sawDanger = true;
    } else if (event.phase === 'round' && event.loop === 'generate') {
      step = Math.max(step, 1) as GenStep;
    } else if (event.phase === 'publishing' || event.phase === 'completed') {
      step = Math.max(step, 4) as GenStep;
    }
  }
  // 事件流缺失（WS 未连接/丢帧）时退化为构建大纲进行中，只转 spinner
  if (events.length === 0) step = 1;
  return { step, repairRounds, sawDanger };
}

/** 一轮工具调用聚合成 `co_patch ×8 ✓ · co_query ✗` 形态 */
export function summarizeTools(tools: ILearningGenerationToolCall[] | undefined): string {
  const counts = new Map<string, { ok: number; failed: number }>();
  for (const call of tools ?? []) {
    const entry = counts.get(call.name) ?? { ok: 0, failed: 0 };
    if (call.is_error) entry.failed += 1;
    else entry.ok += 1;
    counts.set(call.name, entry);
  }
  return [...counts.entries()]
    .map(([name, { ok, failed }]) => {
      const times = ok + failed > 1 ? ` ×${ok + failed}` : '';
      const mark = failed > 0 ? ' ✗' : ' ✓';
      return `${name}${times}${mark}`;
    })
    .join(' · ');
}

/** 课时生成的行内迷你进度行（按 lesson 过滤后的事件 → 一句话）；
 * 终态事件返回 null（清空文本）。 */
export function lessonProgressLine(
  event: ILearningCourseGenerationEvent,
  t: Translate
): string | null {
  if (event.phase === 'round') {
    return `${t('learning.lessonGenRunning')} · ${t('learning.lessonGenRound', { round: event.round ?? '' })}`;
  }
  if (event.phase === 'audit') {
    return `${t('learning.lessonGenRunning')} · ${t('learning.lessonGenAudit', {
      danger: event.danger ?? 0,
      warning: event.warning ?? 0,
    })}`;
  }
  return null;
}

// ── t5 新管线（清点分诊 → 来源结构计划 → 分批展开）─────────────────────

/** 新管线步骤条五段：准备 → 来源分诊 → 来源结构计划 → 课时展开 → 完成。
 * 术语铁律：这里只谈「来源分诊/来源结构计划」，与学习图的
 * Structure/Coverage Gauge 无关。 */
export type ModernGenStep = 0 | 1 | 2 | 3 | 4;

/** 事件流里是否观察到新管线的分诊阶段——决定步骤条用新标签还是旧
 * （描述流/旧客户端走旧五段，零回归）。 */
export function isModernPipeline(events: ILearningCourseGenerationEvent[]): boolean {
  return events.some(
    (event) => event.phase === 'triage_started' || event.phase === 'triage_completed'
  );
}

/** 新管线的事件流折叠：triage_* → 1；source_plan_* → 2；coverage_ready /
 * publishing → 3；completed → 4。空流退化到分诊中（等待提示可见）。 */
export function deriveModernStep(events: ILearningCourseGenerationEvent[]): ModernGenStep {
  let step: ModernGenStep = 0;
  for (const event of events) {
    if (event.phase === 'triage_started' || event.phase === 'triage_completed') {
      step = Math.max(step, 1) as ModernGenStep;
    } else if (
      event.phase === 'source_plan_started' ||
      event.phase === 'source_plan_ready'
    ) {
      step = Math.max(step, 2) as ModernGenStep;
    } else if (event.phase === 'coverage_ready' || event.phase === 'publishing') {
      step = Math.max(step, 3) as ModernGenStep;
    } else if (event.phase === 'completed') {
      step = Math.max(step, 4) as ModernGenStep;
    }
  }
  if (events.length === 0) step = 1;
  return step;
}

/** triage 计数（WS 帧 / DTO 摘要共用形状）的最小读数。 */
export interface TriageCountsLike {
  total: number;
  core: number;
  prerequisite: number;
  practice: number;
  mixed: number;
  noise: number;
  duplicate: number;
  uncertain: number;
}

/** DTO 分诊摘要面（`total_files` 字段）→ 计数行/判定入参（`total` 字段）
 * 的形状适配——WS 帧与 DTO 两个来源共用同一组读数函数。 */
export function triageViewCounts(view: {
  total_files: number;
  core: number;
  prerequisite: number;
  practice: number;
  mixed: number;
  noise: number;
  duplicate: number;
  uncertain: number;
}): TriageCountsLike {
  return {
    total: view.total_files,
    core: view.core,
    prerequisite: view.prerequisite,
    practice: view.practice,
    mixed: view.mixed,
    noise: view.noise,
    duplicate: view.duplicate,
    uncertain: view.uncertain,
  };
}

/** 分诊计数行：「扫描 N 文件 → core X / mixed Y / 跳过 Z / 重复 W /
 * 不确定 U」。跳过 = noise + duplicate（两者都不进课程）。 */
export function triageCountsLine(counts: TriageCountsLike, t: Translate): string {
  return t('learning.genTriageLine', {
    total: counts.total,
    core: counts.core,
    mixed: counts.mixed,
    skipped: counts.noise + counts.duplicate,
    duplicate: counts.duplicate,
    uncertain: counts.uncertain,
  });
}

/** uncertain 过多（空态/提醒文案的触发条件）：数量 ≥10，或超过 core
 * （分诊结果被不确定淹没）——建议先整理知识库，不是硬失败。 */
export function uncertainTooMany(counts: TriageCountsLike): boolean {
  return counts.uncertain >= 10 || (counts.core > 0 && counts.uncertain > counts.core);
}

/** 成本门确认弹窗的读数行：文件数 / 批数 / 调用数 / 预计耗时（分钟为
 * 粗估值，后端按调用数 × 经验常数折算）。 */
export function estimateLines(
  estimate: import('./types').CourseGenerationEstimateView,
  t: Translate
): string[] {
  const minutes = Math.max(1, Math.round(estimate.estimated_seconds / 60));
  return [
    t('learning.costGateFiles', { count: estimate.total_files }),
    t('learning.costGateBatches', { count: estimate.estimated_batches }),
    t('learning.costGateCalls', { count: estimate.estimated_llm_calls }),
    t('learning.costGateDuration', { minutes }),
  ];
}

// ── t5 课程详情「来源覆盖」卡（SourceCoverage；不是学习图的 Coverage Gauge）──

/** 覆盖卡三态：旧课程/描述流（列上 NULL）不显示空卡；未知快照版本显式
 * 「不可用」（有文案不静默）；正常快照展开完整读数。 */
export type SourceCoverageCardState = 'hidden' | 'unavailable' | 'ready';

/** 覆盖卡的三态判定：`source_plan` 整体缺席 = hidden；`snapshot.available
 * = false`（未知版本/坏形状，读侧降级）= unavailable；否则 ready。 */
export function sourceCoverageState(
  plan: import('./types').CourseSourcePlanView | null
): SourceCoverageCardState {
  if (!plan) return 'hidden';
  return plan.snapshot.available ? 'ready' : 'unavailable';
}

/** 覆盖面的 topic 总数读数：covered + skipped + uncertain 标签数（topic
 * 四态里 uncertain 只以标签列表出现，没有独立计数面）。 */
export function coverageTopicTotals(coverage: {
  topics_covered: number;
  topics_skipped: number;
  uncertain?: string[];
}): { covered: number; skipped: number; total: number } {
  const covered = coverage.topics_covered;
  const skipped = coverage.topics_skipped;
  return { covered, skipped, total: covered + skipped + (coverage.uncertain?.length ?? 0) };
}
