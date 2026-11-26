/**
 * 学习模块的纯决策函数层（stepper 构造、节状态语义、锁定推导、掌握度
 * 规则、生成阶段折叠）：零渲染、零副作用，全部可直接单测——组件只做组
 * 合与展示，业务决策都收在这里（避免决策逻辑埋在渲染体里无法测试）。
 */
import type { ILearningCourseGenerationEvent, ILearningGenerationToolCall } from '@/common/adapter/ipcBridge';
import { ACTIVITY_KIND_REGISTRY } from './activityKindRegistry';
import { PHASE } from './phaseMeta';
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

// ── sq3: AI 批改题跳过（activitySkip 语义域）──────────────────────────

/** 跳过标记（sq3 R-a）：AI 批改题的跳过提交载体——与后端
 * `response_marks_skipped` 的判定形状互为镜像，两端同形改一侧须回改另一
 * 侧。提交即记 skipped attempt（零 LLM 判分），响应面以 AttemptResult
 * .skipped 回透。 */
export const ACTIVITY_SKIP_RESPONSE = { skipped: true } as const;

/** 判定一次提交是否为跳过（模型门前置豁免用）：跳过不需要 AI 模型——
 * 未选模型也必须放行跳过，否则跳过这个逃生门自己被模型门锁死。 */
export function isActivitySkipResponse(response: unknown): boolean {
  return (
    typeof response === 'object' &&
    response !== null &&
    !Array.isArray(response) &&
    (response as { skipped?: unknown }).skipped === true
  );
}

// ── 分节 stepper（新规范：练习节 = 一等练习轮；旧清单回退综合步）──────

/** 分节交付的一步：节正文 + 该步的作答轮（练习节）；null = 综合步。sq4
 * 绑定退役：新生成题不再带 section_key 绑定（落库恒 NULL），绑定措辞仅剩
 * 旧清单数据兼容语义。 */
export interface LessonStep {
  section: Section | null;
}

/** 步骤数组：练习节收尾的新规范不含综合步；旧清单（无练习节）且存在
 * section_key===null 的题时在末尾追加综合步，不与单节内容混排。sq4 绑定
 * 退役后新生成课的题全为 null，但新规范恒带练习节（outline 硬契约）——
 * 综合步只对旧清单课时（存量数据）触发，新数据零行为变化。 */
export function buildSteps(sections: Section[], activities: Activity[]): LessonStep[] {
  const hasPractice = sections.some((section) => section.kind === 'practice');
  const hasGeneral = activities.some((activity) => activity.section_key === null);
  return [
    ...sections.map((section) => ({ section })),
    ...(hasPractice || !hasGeneral ? [] : [{ section: null }]),
  ];
}

/** 某一步的作答轮：收尾练习轮（最后一个练习步）收编全部计入题——sq4 绑
 * 定退役前「收尾轮全收编」已是事实（outline 硬契约「恰好一个收尾练习节」
 * 下 earlierPracticeKeys 恒空、全部题归收尾轮），退役后语义显式化：新题
 * section_key 全 NULL，`section_key === null` 臂恒真。内容步与综合步不设
 * 门（门禁只拦练习步，见 ADR-0002 追加决策）。
 *
 * `index !== lastPracticeIndex` 分支（练习节按 section_key 出自己的题）是
 * 旧契约形态——「恰好一个收尾练习节」契约后不再产生，留代码路径兼容旧清
 * 单存量数据（sq4 裁决：少动面）。
 *
 * pf5 注册表派生：`countsTowardSection=false` 的活动（首例 deprecated
 * matching）不入作答轮——练习流不再出现是本过滤的下游结果（Q2 隐藏裁
 * 决不加额外逻辑）；节内全是 deprecated 题时 round 为空，`stepLocked`
 * 的 `round.length > 0` 守卫天然放行 = 全 deprecated 节自动视为完成
 * （Q3 裁决零成本落地）。 */
export function stepActivities(
  steps: LessonStep[],
  index: number,
  activities: Activity[]
): Activity[] {
  const entry = steps[index];
  if (!entry?.section || entry.section.kind !== 'practice') return [];
  const countsTowardSection = (activity: Activity) =>
    ACTIVITY_KIND_REGISTRY[activity.kind].countsTowardSection;
  const lastPracticeIndex = findLastPracticeIndex(steps);
  if (index !== lastPracticeIndex) {
    // 旧契约形态（多练习节）：非收尾练习节出自己绑定的题（存量数据兼容）。
    return activities.filter(
      (activity) =>
        activity.section_key === entry.section?.section_key && countsTowardSection(activity)
    );
  }
  // 收尾轮：除更早练习节已出的题外全部归此（绑定退役后新题全 NULL → 全收编）
  const earlierPracticeKeys = new Set<string>();
  steps.slice(0, lastPracticeIndex).forEach((step) => {
    if (step.section?.kind === 'practice') earlierPracticeKeys.add(step.section.section_key);
  });
  return activities.filter(
    (activity) =>
      (activity.section_key === null || !earlierPracticeKeys.has(activity.section_key)) &&
      countsTowardSection(activity)
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
    if (event.phase === PHASE.round && event.loop === PHASE.repair) {
      step = Math.max(step, 3) as GenStep;
      repairRounds += 1;
    } else if (event.phase === PHASE.audit) {
      step = Math.max(step, 2) as GenStep;
      if ((event.danger ?? 0) > 0) sawDanger = true;
    } else if (event.phase === PHASE.round && event.loop === PHASE.generate) {
      step = Math.max(step, 1) as GenStep;
    } else if (event.phase === PHASE.publishing || event.phase === PHASE.completed) {
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

/** M2 共享耗时后缀：started 起表的耗时在场时统一拼「 · 已 {{elapsed}}」
 * ——plan/section/activities/audit/修复审校/publishing 各相位共用一份拼
 * 装（词条拆基句 + 后缀，不再逐相位复制整句）。elapsed 缺席（started 帧
 * 丢失/订阅中途的降级路径）返回空串——退无耗时文案。 */
function elapsedSuffix(elapsed: string | null, t: Translate): string {
  return elapsed ? ` · ${t('learning.lessonGenElapsed', { elapsed })}` : '';
}

/** 课时生成的行内迷你进度行（按 lesson 过滤后的事件 → 一句话）；
 * 终态与未知事件返回 null（清空文本）。
 *
 * f1 相位化呈现：plan → 「规划节清单」；section → 「写第 {{index}}/
 * {{total}} 节」；activities → 「出题中」；publishing → 「发布中」（发布
 * 帧不再清行）；audit 按环区分（生成环质量检查 / 修复环独立「修复审校」
 * 词条）；裸 round 帧的轮次数字退出学习者面——向后兼容兜底只出通用
 * 「生成中」，不带数字。M2：前述相位全部拼共享耗时后缀（elapsed 缺席退
 * 无耗时），round 兜底不带。 */
export function lessonProgressLine(
  event: ILearningCourseGenerationEvent,
  t: Translate,
  elapsed: string | null = null
): string | null {
  const suffix = elapsedSuffix(elapsed, t);
  if (event.phase === PHASE.plan) {
    return t('learning.lessonGenPlan') + suffix;
  }
  if (event.phase === PHASE.section) {
    const vars = { index: event.index ?? 0, total: event.total ?? 0 };
    return t('learning.lessonGenSection', vars) + suffix;
  }
  if (event.phase === PHASE.activities) {
    return t('learning.lessonGenActivities') + suffix;
  }
  if (event.phase === PHASE.publishing) {
    return t('learning.lessonGenPublishing') + suffix;
  }
  if (event.phase === PHASE.audit) {
    if (event.loop === PHASE.repair) {
      return (
        t('learning.lessonGenAuditRepair', {
          danger: event.danger ?? 0,
          warning: event.warning ?? 0,
        }) + suffix
      );
    }
    return (
      `${t('learning.lessonGenRunning')} · ${t('learning.lessonGenAudit', {
        danger: event.danger ?? 0,
        warning: event.warning ?? 0,
      })}` + suffix
    );
  }
  if (event.phase === PHASE.round) {
    return t('learning.lessonGenRunning');
  }
  return null;
}

// ── t5 新管线（清点分诊 → 来源结构计划 → 分批展开）─────────────────────

/** 新管线步骤条五段：准备 → 来源分诊 → 来源结构计划 → 课时展开 → 完成。
 * 术语铁律：这里只谈「来源分诊/来源结构计划」，与学习图的
 * Structure/Coverage Gauge 无关。 */
export type ModernGenStep = 0 | 1 | 2 | 3 | 4;

/** 事件流里是否观察到新管线的分诊阶段——声明字段优先（x11），无字段时
 * 退回旧嗅探兜底（滚动部署期旧帧混入不炸）。 */
export function isModernPipeline(
  events: ILearningCourseGenerationEvent[],
  declared?: 'sampling' | 'source_plan'
): boolean {
  if (declared === 'source_plan') return true;
  if (declared === 'sampling') return false;
  return events.some(
    (event) => event.phase === PHASE.triageStarted || event.phase === PHASE.triageCompleted
  );
}

/** 新管线的事件流折叠：triage_* → 1；source_plan_* → 2；coverage_ready /
 * publishing → 3；completed → 4。空流退化到分诊中（等待提示可见）。 */
export function deriveModernStep(events: ILearningCourseGenerationEvent[]): ModernGenStep {
  let step: ModernGenStep = 0;
  for (const event of events) {
    if (event.phase === PHASE.triageStarted || event.phase === PHASE.triageCompleted) {
      step = Math.max(step, 1) as ModernGenStep;
    } else if (
      event.phase === PHASE.sourcePlanStarted ||
      event.phase === PHASE.sourcePlanReady
    ) {
      step = Math.max(step, 2) as ModernGenStep;
    } else if (event.phase === PHASE.coverageReady || event.phase === PHASE.publishing) {
      step = Math.max(step, 3) as ModernGenStep;
    } else if (event.phase === PHASE.completed) {
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

/** 分诊计数行（生成进度视图 triageCompleted 帧用）：「已整理 N 个来源文件：
 * 核心 X、跳过 Y」。跳过 = noise + duplicate（两者都不进课程）；混合/重复/
 * 不确定三个残余计数不上主行，收进可展开明细的首行（`triageDetailCountsLine`
 * ——CourseGenerationProgress 的 triage「详情」折叠），文件级列表走折叠内
 * 二级分组。 */
export function triageCountsLine(counts: TriageCountsLike, t: Translate): string {
  return t('learning.genTriageLine', {
    total: counts.total,
    core: counts.core,
    skipped: counts.noise + counts.duplicate,
  });
}

/** 分诊明细首行（F4）：主行只报 total/core/跳过后被丢弃的三个残余计数
 * （混合/重复/不确定）收进「详情」折叠首行——单一模板串，计数无处呈现的
 * mixed 从此有归处。三个计数全为 0 时返回 null（不渲染「0、0、0」噪声行）。 */
export function triageDetailCountsLine(counts: TriageCountsLike, t: Translate): string | null {
  if (counts.mixed === 0 && counts.duplicate === 0 && counts.uncertain === 0) return null;
  return t('learning.genTriageDetailCounts', {
    mixed: counts.mixed,
    duplicate: counts.duplicate,
    uncertain: counts.uncertain,
  });
}

/** uncertain 过多（空态/提醒文案的触发条件）：数量 ≥10，或超过 core
 * （分诊结果被不确定淹没）——建议先整理知识库，不是硬失败。 */
export function uncertainTooMany(counts: TriageCountsLike): boolean {
  return counts.uncertain >= 10 || (counts.core > 0 && counts.uncertain > counts.core);
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

// ── o4：来源分诊 role 枚举 → 人面角色名（机器枚举不再上屏）──────────────

/** 后端 `KbTriageRole`（triage.rs，serde snake_case）→ 词典 key。七档全覆盖；
 * 未知档位（后端新增）走 `sourceCoverageRoleOther` 兜底，不落机器名。 */
export const SOURCE_ROLE_KEYS: Record<string, `learning.sourceCoverageRole${string}`> = {
  core: 'learning.sourceCoverageRoleCore',
  prerequisite: 'learning.sourceCoverageRolePrerequisite',
  practice: 'learning.sourceCoverageRolePractice',
  mixed: 'learning.sourceCoverageRoleMixed',
  noise: 'learning.sourceCoverageRoleNoise',
  duplicate: 'learning.sourceCoverageRoleDuplicate',
  uncertain: 'learning.sourceCoverageRoleUncertain',
};

/** role 枚举 → 角色名（被排除/重复文件诊断行共用一份口径）。 */
export function sourceRoleLabel(role: string, t: Translate): string {
  const key = SOURCE_ROLE_KEYS[role] ?? 'learning.sourceCoverageRoleOther';
  return t(key);
}

// ── f2：课程进度分母（剔除 skipped）──────────────────────────────────

/** 进度读数入参面：CourseSummary 的最小投影（CourseDetail.course 同形，
 * 课程卡与工作区大纲头共用同一组读数函数）。 */
export interface ProgressCountersLike {
  total_lessons: number;
  skipped_lessons: number;
}

/** 进度分母（f2 裁决 A）：可计课时 = total − skipped，钳 ≥0。被跳过的
 * 课时退出分母（跳过 ≠ 完成，永不进分子）；unskip 自动回分母；全跳 →
 * 0 = 无可计进度（GLOSSARY「进度」「跳过」词条口径）。 */
export function progressDenominator(course: ProgressCountersLike): number {
  return Math.max(0, course.total_lessons - course.skipped_lessons);
}

/** 进度百分比：分子 ÷ 可计分母，四舍五入取整；无可计课时（分母 0）
 * 返回 null——消费面呈「—」（em-dash）且不渲染进度条。pg4 起消费面统一
 * 传 `settled_or_completed`（后端在结算账本为空时回退 completed_lessons），
 * 文本与进度条同口径。 */
export function progressPercent(numerator: number, denominator: number): number | null {
  if (denominator <= 0) return null;
  return Math.round((numerator / denominator) * 100);
}

/** S3 进度读数文本（共享展示面）：a/N 计数行，分母 0 → 「—」。分子作参数
 * ——CourseCard 与 CourseWorkspace 都传 `settled_or_completed`（pg4：与
 * percent 同口径；后端在账本为空时回退 completed_lessons）。尾缀供
 * CourseCard 追加课时单位词条，只在 a/N 臂拼（「—」臂不带）。 */
export function progressReadoutText(
  numerator: number,
  denominator: number,
  suffix = ''
): string {
  return denominator <= 0 ? '—' : `${numerator}/${denominator}${suffix}`;
}
