/**
 * 学习模块的纯决策函数层（stepper 构造、节状态语义、锁定推导、掌握度
 * 规则、生成阶段折叠）：零渲染、零副作用，全部可直接单测——组件只做组
 * 合与展示，业务决策都收在这里（避免决策逻辑埋在渲染体里无法测试）。
 */
import type { ILearningCourseGenerationEvent, ILearningGenerationToolCall } from '@/common/adapter/ipcBridge';
import type { Activity, LessonStatus, Section } from './types';
import type { Translate } from './utils';

/** 全部概念掌握的达标线：每个概念的 mastery ≥ 80% 视为「全部掌握」 */
export const MASTERY_THRESHOLD = 0.8;

/** 课程掌握度判定：无概念不算掌握，任一概念未达标即未全掌握 */
export function allConceptsMastered(concepts: { mastery: number | null }[]): boolean {
  return (
    concepts.length > 0 &&
    concepts.every((concept) => concept.mastery !== null && concept.mastery >= MASTERY_THRESHOLD)
  );
}

/** 课时状态 → Arco Tag 颜色（课程大纲/工作区/学习图详情共用） */
export const lessonStatusTagColors: Record<LessonStatus, string> = {
  not_started: 'gray',
  in_progress: 'blue',
  completed: 'green',
  skipped: 'gray',
};

/**
 * 学习进度五态色板（Arco 语义 token，随主题切换）：not_started 中性、
 * in_progress 主色、completed 成功绿、skipped 灰（已声明掌握）、
 * recommended 琥珀（下一步推荐）。DAG 节点色条/描边与 MiniMap 共用。
 * 注意：主题体系未导出 warning/primary/success 的 `-6` 色阶变量，必须带
 * 字面 fallback——SVG stroke 的 var() 失效时会 fallback 到初始值 none，
 * 连线直接消失（HTML 属性只是变色，不会消失）。
 */
export const lessonStatusAccents: Record<LessonStatus, string> = {
  not_started: 'var(--color-text-4)',
  in_progress: 'var(--color-primary-6, #165dff)',
  completed: 'var(--color-success-6, #00b42a)',
  skipped: 'var(--color-text-3)',
};

/** 推荐节点的琥珀强调色（与五态色板同源的 fallback 约束） */
export const RECOMMENDED_ACCENT = 'var(--color-warning-6, #ff7d00)';

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

/** 解锁 = 不存在任何「未完成且未跳过」的前置（根节点天然解锁）。从边表
 * 推导：完成/跳过集合 ∩ 入边 from，命中即锁定 to。 */
export function prereqLockedIds(
  nodes: { lesson_id: string; status: LessonStatus }[],
  edges: { from: string; to: string }[]
): Set<string> {
  const locked = new Set<string>();
  const satisfied = new Set(
    nodes
      .filter((node) => node.status === 'completed' || node.status === 'skipped')
      .map((node) => node.lesson_id)
  );
  for (const edge of edges) {
    if (!satisfied.has(edge.from)) locked.add(edge.to);
  }
  return locked;
}

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
