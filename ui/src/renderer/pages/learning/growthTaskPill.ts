import type { LearningGenerationTaskView } from './types';

/**
 * 生长全局悬浮 Pill 的纯状态面（g4，推广建课链 CourseGenerationPill 模式）。
 *
 * 状态纪律与建课链同款：
 * - **轮询为准**：`GET /api/learning/generation-tasks` 快照决定 Pill 的入列、
 *   进度与终态；快照里没有的任务不臆造终态（直接出列）。
 * - **WS 仅增强**：`learning.course-generation` 的 growth_* 帧只触发提前
 *   轮询与「打回重起草」计数——WS 不重放不补发，任何帧都不判终态。
 */

/** 快照 `kind` 的生长任务机器名（后端 KIND_GROWTH）。 */
export const GROWTH_TASK_KIND = 'growth';

/** 有活跃项时的轮询间隔（与课时任务轮询 pollLessonTask 同节奏）。 */
export const GROWTH_POLL_ACTIVE_MS = 2_000;
/** 无活跃项时的兜底轮询间隔：WS 断线漏帧时也能发现新开跑的生长。 */
export const GROWTH_POLL_IDLE_MS = 15_000;

export type GrowthPillStatus = 'queued' | 'running' | 'done' | 'failed' | 'cancelled';

/** WS 增强读数：本轮被门/评审打回后重起草的累计次数与最近一次的关口。 */
export interface GrowthPillRedraft {
  stage: 'gates' | 'concept_review';
  count: number;
}

export interface GrowthPillItem {
  /** 任务键 `growth:{course_id}:-`（取消端点按键寻址） */
  key: string;
  courseId: string;
  /** 后端主题原文（`学习图生长 · {课程标题}`） */
  topic: string;
  status: GrowthPillStatus;
  /** 机器面步骤常量（如 `draft`） */
  step: string | null;
  /** 人面单句（后端单源；失败时即失败原因） */
  detail: string | null;
  elapsedSecs: number | null;
  queuePosition: number | null;
  /** 仅 WS 增强（可缺席）；不参与任何状态判定 */
  redraft: GrowthPillRedraft | null;
}

/** `learning.course-generation` 帧里本模块读取的字段子集。 */
export interface GrowthEventFrame {
  course_id?: string;
  event?: string;
  stage?: string;
}

const STATUSES: readonly GrowthPillStatus[] = ['queued', 'running', 'done', 'failed', 'cancelled'];

function parseStatus(state: string): GrowthPillStatus | null {
  return (STATUSES as readonly string[]).includes(state) ? (state as GrowthPillStatus) : null;
}

/** 排队中/运行中 = 活跃（Pill 的入列条件；可取消） */
export function isGrowthActive(status: GrowthPillStatus): boolean {
  return status === 'queued' || status === 'running';
}

function fromRow(
  row: LearningGenerationTaskView,
  status: GrowthPillStatus,
  redraft: GrowthPillRedraft | null = null
): GrowthPillItem {
  return {
    key: row.key,
    courseId: row.course_id,
    topic: row.topic,
    status,
    step: row.step,
    detail: row.detail,
    elapsedSecs: row.elapsed_secs,
    queuePosition: row.queue_position,
    redraft,
  };
}

/**
 * 把一次快照并入已跟踪列表：活跃的 growth 行入列；已跟踪项按快照刷新
 * （含终态——终态以 HTTP 为准）；从未跟踪过的终态行不入列（不复活历史）。
 */
export function mergeGrowthSnapshot(
  tracked: readonly GrowthPillItem[],
  snapshot: readonly LearningGenerationTaskView[]
): GrowthPillItem[] {
  const rows = new Map<string, LearningGenerationTaskView>();
  for (const row of snapshot) {
    if (row.kind === GROWTH_TASK_KIND) rows.set(row.key, row);
  }
  const next: GrowthPillItem[] = [];
  for (const item of tracked) {
    const row = rows.get(item.key);
    rows.delete(item.key);
    // 快照缺行（如后端重启清空注册表）：HTTP 没有终态就不臆造，直接出列。
    if (!row) continue;
    const status = parseStatus(row.state);
    // 未知状态（后端新增态）：保留上一次读数，等下一次快照。
    if (!status) {
      next.push(item);
      continue;
    }
    // 增强读数只随同一轮活跃期延续：到终态清零（新一轮从零计）。
    const carried =
      isGrowthActive(status) && isGrowthActive(item.status) ? item.redraft : null;
    next.push(fromRow(row, status, carried));
  }
  for (const row of rows.values()) {
    const status = parseStatus(row.state);
    if (status && isGrowthActive(status)) next.push(fromRow(row, status));
  }
  return next;
}

/**
 * 并入一帧 WS 事件（仅增强）：`growth_redraft` 给该课程已跟踪的活跃项累计
 * 重起草次数；其余帧（含 completed/failed/cancelled）不改任何状态——调用方
 * 收到 growth_* 帧应立即补一次轮询，终态由快照落定。未跟踪课程的帧不入列。
 */
export function applyGrowthEvent(
  tracked: readonly GrowthPillItem[],
  frame: GrowthEventFrame
): GrowthPillItem[] {
  if (frame.event !== 'growth_redraft') return [...tracked];
  const stage = frame.stage === 'concept_review' ? 'concept_review' : 'gates';
  return tracked.map((item) =>
    item.courseId === frame.course_id && isGrowthActive(item.status)
      ? { ...item, redraft: { stage, count: (item.redraft?.count ?? 0) + 1 } }
      : item
  );
}

/** 关闭一个终态项（看过结果）。活跃项不可关闭——它仍在跑，下次快照会带回。 */
export function dismissGrowthItem(
  tracked: readonly GrowthPillItem[],
  key: string
): GrowthPillItem[] {
  return tracked.filter((item) => item.key !== key || isGrowthActive(item.status));
}

/** 下一次轮询的间隔：有活跃项走快轮询，否则慢轮询兜底。 */
export function growthPollDelay(tracked: readonly GrowthPillItem[]): number {
  return tracked.some((item) => isGrowthActive(item.status))
    ? GROWTH_POLL_ACTIVE_MS
    : GROWTH_POLL_IDLE_MS;
}

/** 后端生长任务主题前缀（begin_growth_task 的 `学习图生长 · {title}`）。 */
const GROWTH_TOPIC_PREFIX = '学习图生长 · ';

/** 主题 → 课程名：Pill 主行已说明「学习图生长」，副行只留课程名；前缀
 * 对不上（后端改了措辞）就原样展示，不丢信息。 */
export function growthCourseTitle(topic: string): string {
  return topic.startsWith(GROWTH_TOPIC_PREFIX) ? topic.slice(GROWTH_TOPIC_PREFIX.length) : topic;
}

/** 已运行时长：`m:ss`，满一小时 `h:mm:ss`（语言无关读数）。 */
export function formatGrowthElapsed(totalSecs: number): string {
  const secs = Math.max(0, Math.floor(totalSecs));
  const hours = Math.floor(secs / 3600);
  const minutes = Math.floor((secs % 3600) / 60);
  const seconds = String(secs % 60).padStart(2, '0');
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, '0')}:${seconds}`
    : `${minutes}:${seconds}`;
}
