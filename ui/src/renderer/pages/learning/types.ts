export type ActivityKind =
  | 'single_choice'
  | 'true_false'
  | 'reflection'
  | 'fill_in_blank'
  | 'multi_choice'
  | 'numeric'
  | 'ordering'
  | 'matching'
  | 'open_question';

/** 课时内部的节段类型（ADR-0002 首期 5 种；交互节暂缓） */
export type SectionKind = 'concept' | 'example' | 'demo' | 'summary' | 'practice';

/** 分节正文：body_md 自带 `## ` 标题行，可直接渲染 */
export interface Section {
  section_key: string;
  kind: SectionKind;
  title: string;
  points: string;
  body_md: string;
  /** 当前正文是否为降级纯文字兜底（visual 承诺未兑现，ADR-0008） */
  degraded?: boolean;
  status: 'pending' | 'ready' | 'failed';
  version: number;
  position: number;
}

/** 课程讲解风格（课程级选择，决定节写作提示词变体） */
export type TeachingStyle = 'standard' | 'socratic' | 'feynman';
export type LessonStatus = 'not_started' | 'in_progress' | 'completed' | 'skipped';
export type ReviewRating = 'again' | 'hard' | 'good' | 'easy';
export type ReviewSource = 'course' | 'custom';
export type QuestionState = 'unlearned' | 'new' | 'due' | 'scheduled' | 'archived';
/** 课程类型：传统课程（大纲驱动）与学习图（beta，前置网络驱动） */
export type CourseKind = 'traditional' | 'learning_graph';

/** 生成课程请求：知识库流与描述流二选一（都传时后端以知识库为准）。
 * 学习图课程只走描述流（描述即学习目标），endpoints 为用户确认过的终点锚。 */
export interface GenerateCourseRequest {
  course_kind?: CourseKind;
  knowledge_base_id?: string;
  description?: string;
  domain?: string;
  provider_id?: string;
  model?: string;
  /** 讲解风格（课程级）：standard 标准 / socratic 苏格拉底 / feynman 费曼 */
  teaching_style?: TeachingStyle;
  /** 学习图课程的初始终点锚（为空时由 AI 提议） */
  endpoints?: EndpointInput[];
}

/** 学习图生成状态（后台指示条/取消入口的数据源）。创建在请求内完成，
 * 生长在后台任务执行——注册表让运行对外可发现、可取消。 */
export interface LearningGraphGenerationStatus {
  running: boolean;
  topic: string | null;
  elapsed_secs: number | null;
}

/** 终点锚提议请求（建课向导第二步的 AI 提议） */
export interface ProposeEndpointsRequest {
  description: string;
  provider_id?: string;
  model?: string;
}

/** AI 提议的一条终点锚 */
export interface ProposedEndpointView {
  title: string;
  goal_note: string;
}

/** 按需生成单个课时内容时可选的模型偏好；两个字段同时传或不传。
 * feedback 是单节重写的可选学习建议（ADR-0007），为空即同分布重生成。 */
export interface GenerateLessonRequest {
  provider_id?: string;
  model?: string;
  feedback?: string;
}

/** 手动编辑节正文请求（ADR-0007）：仅覆盖 body_md */
export interface UpdateSectionBodyRequest {
  body_md: string;
}

export interface CourseSummary {
  id: string;
  title: string;
  description: string;
  domain: string;
  source_kb_id: string | null;
  version: number;
  enrolled: boolean;
  total_lessons: number;
  completed_lessons: number;
  updated_at: number;
  tags: string[];
  course_kind: CourseKind;
}

export interface Activity {
  id: string;
  kind: ActivityKind;
  prompt: string;
  options: string[];
  /** matching 题的右列候选（按存储顺序；前端渲染时本地打乱） */
  matches: string[];
  /** 来源节 key；null = 跨节综合题（通用） */
  section_key: string | null;
  position: number;
}

export interface Lesson {
  id: string;
  title: string;
  summary: string;
  purpose: string;
  position: number;
  generated: boolean;
  estimated_minutes: number;
  source: { path: string; start: number | null; end: number | null } | null;
  status: LessonStatus;
  activities: Activity[];
  /** 分节正文；空 = 旧课时的单篇 summary（双读回退） */
  sections: Section[];
}

export interface LearningModule {
  id: string;
  title: string;
  description: string;
  position: number;
  lessons: Lesson[];
}

export interface CourseDetail {
  course: CourseSummary;
  enrollment_id: string | null;
  modules: LearningModule[];
  next_lesson_id: string | null;
  due_review_count: number;
  /** 仅 learning_graph 课程携带：终点锚 + 罗盘 + 就绪集（ADR-0009） */
  graph: LearningGraphView | null;
}

export interface AttemptResult {
  id: string;
  score: number;
  passed: boolean;
  feedback: string;
}

/** 作答记录 = 判卷结果 + 提交的原始作答；response 用于回看时回显用户答案 */
export interface AttemptRecord extends AttemptResult {
  response?: unknown;
}

/** 活动作答提交。reflection 批改可携带显式模型偏好；未携带时后端回落默认模型 */
export interface SubmitAttemptRequest {
  response: unknown;
  provider_id?: string;
  model?: string;
}

export interface ReviewQuestion {
  activity_id: string | null;
  kind: ActivityKind;
  prompt: string;
  options: string[];
  matches: string[];
}

export interface DueReview {
  id: string;
  source: ReviewSource;
  enrollment_id: string | null;
  course_id: string | null;
  course_title: string | null;
  module_title: string | null;
  lesson_title: string | null;
  question: ReviewQuestion;
  due_at: number;
  stability_days: number;
  difficulty: number;
  review_count: number;
  lapse_count: number;
  /** FSRS 预测回忆率（0-1）；从未推进过的卡为 null */
  r: number | null;
  /** 已标记“待编辑”，刷卡时记录，不打断复习；描述用于找回思路 */
  edit_pending: boolean;
  edit_note: string | null;
}

export interface ReviewResult {
  id: string;
  due_at: number;
  stability_days: number;
  difficulty: number;
  /** 本次评分是否真实推进了排期；被到期门拦下的过期重复为 false */
  advanced: boolean;
  review_count: number;
  lapse_count: number;
}

/** 当日打卡快照（对齐后端 CheckinStatus）：复习日、目标、进度与锁定状态 */
export interface CheckinStatus {
  /** 本地复习日 YYYYMMDD（02:00 日界线） */
  review_day: number;
  /** 每日复习目标（0 = 仅清空队列） */
  goal: number;
  /** 本复习日已提交的复习数 */
  reviewed_count: number;
  /** 当前到期卡片数（课程 + 自定义） */
  due_count: number;
  /** 当日是否已锁定为完成 */
  completed: boolean;
  /** 完成锁定时刻（UTC 毫秒），未完成时为 null */
  locked_at: number | null;
}

/** 复习日内完成的课时（日历明细） */
export interface CalendarLessonRef {
  lesson_id: string;
  title: string;
}

/** 复习日内创建的课程（日历明细） */
export interface CalendarCourseRef {
  course_id: string;
  title: string;
}

/** 请求范围内的一个复习日，无活动时后端补零 */
export interface CalendarDayStats {
  review_day: number;
  reviewed_count: number;
  checkin_completed: boolean;
  /** 当日到期卡片数（过期卡片并入当天，与复习队列同口径） */
  due_count: number;
  completed_lessons: CalendarLessonRef[];
  created_courses: CalendarCourseRef[];
}

/** 日历聚合响应：月视图或年视图 + 当前 streak */
export interface CalendarStats {
  year: number;
  /** 1..=12 为月视图，null 为年视图 */
  month: number | null;
  tz_offset: number;
  /** 以当前复习日为终点的连续打卡天数；今日未完成时为 0 */
  streak: number;
  days: CalendarDayStats[];
}

export interface ReviewAnswerResult {
  correct: boolean;
  feedback: string;
  correct_answer: unknown | null;
  rated: ReviewResult | null;
  /** 本次作答是否真实推进了排期；被到期门拦下的重复作答为 false */
  advanced: boolean;
}

export interface QuestionEntry {
  source: ReviewSource;
  question_id: string;
  review_item_id: string | null;
  state: QuestionState;
  course_id: string | null;
  course_title: string | null;
  question_kind: ActivityKind | null;
  prompt: string | null;
  options: string[];
  answer: unknown | null;
  /** 填空题的近义干扰项，仅用于展示 */
  distractors: string[];
  explanation: string | null;
  due_at: number | null;
  overdue: boolean;
  stability_days: number;
  difficulty: number;
  review_count: number;
  lapse_count: number;
  last_reviewed_at: number | null;
  updated_at: number;
  tags: string[];
  /** 已标记“待编辑”时的描述，用于找回编辑思路 */
  edit_pending: boolean;
  edit_note: string | null;
}

export interface UpdateQuestionRequest {
  prompt: string;
  options?: string[];
  answer: unknown;
  explanation?: string;
  /** 填空题的近义干扰项（可选，仅填空题型使用） */
  distractors?: string[];
}

export interface CreateCustomQuestionRequest {
  kind: Exclude<ActivityKind, 'reflection'>;
  prompt: string;
  options?: string[];
  answer: unknown;
  explanation?: string;
  /** 填空题的近义干扰项（可选，仅填空题型使用） */
  distractors?: string[];
}

/** 手动向课时追加练习（题型全支持） */
export interface CreateLessonActivityRequest {
  kind: ActivityKind;
  prompt: string;
  options?: string[];
  answer: unknown;
  explanation?: string;
  /** 填空题的近义干扰项（可选，仅填空题型使用） */
  distractors?: string[];
}

/** AI 生成课时练习草案请求（不落库）；provider_id 与 model 同时传或不传 */
export interface GenerateLessonActivityRequest {
  kind: ActivityKind;
  provider_id?: string;
  model?: string;
  /** 可选：用户指定的侧重方向 */
  focus?: string;
}

/** AI 生成的草案（供前端预览确认） */
export interface GeneratedLessonActivity {
  kind: ActivityKind;
  prompt: string;
  options: string[];
  answer: unknown;
  explanation: string;
  distractors: string[];
}

export interface SetTagsRequest {
  tags: string[];
  apply_to_children?: boolean;
}

// ── 学习图（beta，概念网 + 生长模型，ADR-0009） ─────────────────────────

/** 概念掌握档位：知道 < 会用 < 能教 */
export type ConceptTier = 'know' | 'apply' | 'teach';

/** 终点锚：标题 + 一句程度声明；lesson_id 是它的零正文标记课时行 */
export interface GraphEndpointView {
  endpoint_id: string;
  lesson_id: string;
  title: string;
  goal_note: string;
  completed: boolean;
  declared_at: number;
}

/** 终点锚创建/编辑输入 */
export interface EndpointInput {
  title: string;
  goal_note?: string;
}

/** 终点锚部分编辑输入：undefined = 不改动 */
export interface EndpointUpdateInput {
  title?: string;
  goal_note?: string;
}

/** 图视图（挂在 CourseDetail.graph 下）：终点锚 + 罗盘 + 就绪集与水位。
 * 没有先修边、没有锁定态——发布即就绪。 */
export interface LearningGraphView {
  goal: string;
  scope: string;
  /** 罗盘（逐终点剩余路线摘要），终点变更时重画；null = 尚未画出 */
  compass: string | null;
  compass_updated_at: number | null;
  endpoints: GraphEndpointView[];
  /** 下一步推荐学习的节点（就绪集 ≤10） */
  recommended: string[];
  /** 当前就绪存量与水位契约（补货目标 7 / 自动触发线 3） */
  ready_count: number;
  ready_target: number;
  ready_trigger: number;
  /** 是否有生成/生长运行进行中 */
  growth_running: boolean;
}

/** 学习记录视图：批次时间线（倒序）——生长史即课程史 */
export interface GraphHistoryView {
  batches: GraphBatchView[];
}

/** 一个生长批次的出生档案：序号/批注/时刻 + 节点行（含学习者进度） */
export interface GraphBatchView {
  batch_id: string;
  seq: number;
  note: string;
  created_at: number;
  nodes: GraphNodeHistoryView[];
}

export interface GraphNodeHistoryView {
  lesson_id: string;
  title: string;
  estimated_minutes: number;
  status: LessonStatus;
  completed_at: number | null;
}

/** 概念表行：登记表概念 + 本课程的教/假定引用 + 跨课程来源标注 */
export interface GraphConceptRowView {
  concept_id: string;
  canonical: string;
  aliases: string[];
  definition: string;
  /** 本课程内教/假定该概念的节点（带学习者进度状态） */
  refs: GraphConceptRefView[];
  /** 其他还在教该概念的课程标题（跨课程就绪的来源可见性） */
  other_courses: string[];
}

export interface GraphConceptRefView {
  lesson_id: string;
  title: string;
  /** teaches | assumes */
  role: 'teaches' | 'assumes';
  /** know | apply | teach */
  tier: ConceptTier;
  status: LessonStatus;
}

/** 记忆健康面板：到期预报、卡池状态、真实保留率与预测对照/遗忘曲线 */
export interface MemoryLoadDay {
  review_day: number;
  due_count: number;
}

export interface MemoryStateBucket {
  key: 'new' | 'young' | 'mature' | 'master';
  count: number;
}

export interface MemoryTrueRetention {
  passes: number;
  fails: number;
  rate: number | null;
}

export interface MemoryCalibrationBin {
  bucket: number;
  min: number;
  max: number;
  predicted: number;
  actual: number | null;
  count: number;
}

export interface MemoryCurvePoint {
  elapsed_days: number;
  predicted: number;
  actual: number | null;
  count: number;
}

export interface MemoryHealthStats {
  review_day: number;
  tz_offset: number;
  overdue_count: number;
  load_forecast: MemoryLoadDay[];
  state_distribution: MemoryStateBucket[];
  true_retention: MemoryTrueRetention | null;
  calibration: MemoryCalibrationBin[];
  forgetting_curve: MemoryCurvePoint[];
}
