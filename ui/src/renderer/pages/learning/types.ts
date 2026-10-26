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
 * 学习图课程只走描述流（描述即学习目标），程度/基础声明可选（缺省由结构
 * 生成扩写 / 按纯零基础画地形）。
 * 多知识库（t4）：`knowledge_base_ids` 是来源库列表（首个 = 主来源，上限 5），
 * 与单 `knowledge_base_id` 兼容（列表优先；两者同时给出时单值必须是列表成员）。 */
export interface GenerateCourseRequest {
  course_kind?: CourseKind;
  knowledge_base_id?: string;
  /** 多知识库来源列表（首个 = 主来源；上限 5） */
  knowledge_base_ids?: string[];
  description?: string;
  /** 用户目标（知识库流）：整体文本即目标——简单时即课程名，复杂时是目标
   * 描述；课程标题默认即该文本。旧调用不传，缺省回退库名。 */
  goal?: string;
  domain?: string;
  provider_id?: string;
  model?: string;
  /** 讲解风格（课程级）：standard 标准 / socratic 苏格拉底 / feynman 费曼 */
  teaching_style?: TeachingStyle;
  /** 学习图课程的程度声明（可选）：达成后能独立做成什么；缺省由结构生成依据目标扩写 */
  proficiency_statement?: string;
  /** 学习图课程的学习者基础声明（可选）：缺省按纯零基础画地形 */
  learner_basis?: string;
}

/** 学习图生成状态（后台指示条/取消入口的数据源）。创建在请求内完成，
 * 生长在后台任务执行——注册表让运行对外可发现、可取消。 */
export interface LearningGraphGenerationStatus {
  running: boolean;
  topic: string | null;
  elapsed_secs: number | null;
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

/** 目标感知采样的披露（t2）：已采/未采计数与低分未采样示例；无目标路径
 * （字母序回退）与旧课程为 null。完整文件树只在后端留档。
 * `goal_fallback` = 目标与库无交集，采样退回默认顺序（披露不得冒充按目标采样）。 */
export interface CourseSamplingView {
  goal: string | null;
  total_files: number;
  sampled_files: number;
  coherence_files: number;
  goal_fallback: boolean;
  low_score_files: number;
  low_score_examples: string[];
  /** 多知识库（t4）：逐库归属与计数（单库/旧课程为空数组）；namespace 是
   * 采样路径的前缀（`{namespace}/rel`），路径归属与「来源：库A、库B」由它解析 */
  bases: CourseSamplingBaseView[];
}

/** 单库的采样归属（t4 披露条目） */
export interface CourseSamplingBaseView {
  kb_id: string;
  name: string;
  namespace: string;
  total_files: number;
  sampled_files: number;
  /** 该库目标零命中：采样退回默认顺序 */
  goal_fallback: boolean;
}

// ── t5：来源计划（SourcePlan）/ 来源覆盖（SourceCoverage）的读侧视图 ────
// 术语铁律：这里只谈「来源结构计划 / 来源覆盖」，与学习图的
// Structure / Coverage Gauge 是两回事，禁止混用。

/** 来源计划快照的版本面：available=false = 未知版本/坏形状（读侧降级，
 * 计数不可信置零；课程本身照常可学）。 */
export interface CourseSourcePlanSnapshotView {
  version: number | null;
  available: boolean;
}

/** 分诊摘要：七角色计数（总数守恒：各角色之和 = total_files）+ degraded
 * （规则分诊，未做 LLM 判定）。 */
export interface CourseSourcePlanTriageView {
  total_files: number;
  core: number;
  prerequisite: number;
  practice: number;
  mixed: number;
  noise: number;
  duplicate: number;
  uncertain: number;
  degraded: boolean;
  warnings?: string[];
}

/** t2 计划一章的来源映射摘要（章 → 概念/课时/来源文件；零课时章显式
 * skipped + 理由）。 */
export interface CourseSourcePlanChapterSummaryView {
  title: string;
  concepts: number;
  lessons: number;
  skipped: boolean;
  skip_reason: string | null;
  source_files: number;
}

/** 计划摘要：章/概念/边计数 + 证据 gaps + 章节映射摘要。 */
export interface CourseSourcePlanPlanView {
  chapters: number;
  concepts: number;
  edges: number;
  gaps?: string[];
  chapter_summaries: CourseSourcePlanChapterSummaryView[];
  warnings?: string[];
}

/** 被显式跳过的 topic 示例（label + 理由）。 */
export interface CourseSourcePlanSkippedTopicView {
  label: string;
  reason: string | null;
}

/** 展开覆盖摘要：topic 四态 + 前置/练习去向 + 锚点不确定 + 兜底课时。 */
export interface CourseSourcePlanCoverageView {
  topics_covered: number;
  topics_skipped: number;
  prerequisite_topics: number;
  practice_topics: number;
  anchor_uncertain_topics: number;
  uncertain?: string[];
  skipped_examples?: CourseSourcePlanSkippedTopicView[];
  fallback_lessons: number;
  warnings?: string[];
}

/** 一条重复/翻译副本示例（path + 指向的 canonical + canonical 入选理由）。 */
export interface CourseSourcePlanDuplicateExampleView {
  path: string;
  duplicate_of: string;
  canonical_reason: string;
}

/** 重复面：计数（全量）+ 示例（≤20）。 */
export interface CourseSourcePlanDuplicatesView {
  count: number;
  examples: CourseSourcePlanDuplicateExampleView[];
}

/** 来源计划快照的摘要视图（t5 细化分面）：列表/详情共用；旧课程/描述流
 * 为 null（字段不出现，UI 不显示空卡）。 */
export interface CourseSourcePlanView {
  snapshot: CourseSourcePlanSnapshotView;
  triage: CourseSourcePlanTriageView;
  plan: CourseSourcePlanPlanView;
  coverage: CourseSourcePlanCoverageView;
  duplicates: CourseSourcePlanDuplicatesView;
}

/** 一条被排除出候选集的文件（noise/duplicate，path + 角色名 + 理由）。 */
export interface CourseSourcePlanExcludedView {
  path: string;
  role: string;
  reason: string;
}

/** 详情端点独有的诊断明细：全量 excluded/duplicates（限长 200）——列表
 * 摘要只给计数/示例，详情页看完整「为什么没进课程」。 */
export interface CourseSourcePlanDetailView {
  excluded: CourseSourcePlanExcludedView[];
  duplicates: CourseSourcePlanDuplicateExampleView[];
}

/** t5 成本门估算：提交建课前的一次规则分诊读数（零 LLM、degraded=true）。
 * needs_confirm 由后端按 >40 文件或 >20 次预估调用判定（阈值单源）。 */
export interface CourseGenerationEstimateView {
  total_files: number;
  estimated_batches: number;
  estimated_llm_calls: number;
  /** 粗估耗时（秒；按调用数 × 经验常数，仅供确认弹窗展示）。 */
  estimated_seconds: number;
  needs_confirm: boolean;
  triage: CourseSourcePlanTriageView;
  skipped_examples?: CourseSourcePlanExcludedView[];
}

export interface CourseSummary {
  id: string;
  title: string;
  description: string;
  domain: string;
  /** 主来源（t4）：列表首个现存库；无来源/已删库为 null */
  source_kb_id: string | null;
  /** 来源库列表（t4，读侧已剔除已删库 id；空数组 = 描述流或旧课程无来源） */
  source_kb_ids: string[];
  version: number;
  enrolled: boolean;
  total_lessons: number;
  completed_lessons: number;
  /** 课程级「现存且启用」（三轮 g3）：false = 已归档（可逆暂停），安排面不再出现本课 */
  enabled: boolean;
  /** 概况：已结算或已完成的课时/节点数（XP 账本落地前 = completed_lessons） */
  settled_or_completed: number;
  /** 概况：今日到期题数 */
  due_today: number;
  /** 概况：累计 XP（结算账本 j05 落地前为 null） */
  total_xp: number | null;
  updated_at: number;
  tags: string[];
  course_kind: CourseKind;
  /** 用户目标（t1）：知识库流的整体目标文本；旧课程/描述流为空即 null。 */
  learning_goal: string | null;
  /** 目标感知采样的披露（t2）：无目标路径/旧课程为 null。 */
  sampling: CourseSamplingView | null;
  /** 来源计划快照摘要（t5）：旧课程/描述流/未知版本降级见 snapshot.available；
   * 列上 NULL 时为 null（字段不出现，UI 不显示空卡）。 */
  source_plan: CourseSourcePlanView | null;
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
  /** est：专注工作量尺寸锚（1 est 分钟 = 1 分钟专注；后端 DB 列保留旧名 estimated_minutes） */
  est: number;
  /** 主来源（兼容位，= `sources` 首个）：`kb_id` = 来源库（多库课程按命名
   * 空间解析；单库/旧课程为 null，读侧回退课程主来源 `course.source_kb_id`）；
   * `path` 是库内相对路径 */
  source: { path: string; start: number | null; end: number | null; kb_id: string | null } | null;
  /** 全部引用来源（t4 多引用）：位序即引用顺序，首个 = 主来源；空 = 无引用 */
  sources: { path: string; start: number | null; end: number | null; kb_id: string | null }[];
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
  /** 来源计划诊断明细（t5）：全量 excluded/duplicates（限长）；只挂详情
   * 端点，旧课程/描述流为 null。 */
  source_plan_detail?: CourseSourcePlanDetailView | null;
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

/** 当日 XP 快照（对齐后端 CheckinStatus，j05 起打卡按 XP 口径）：复习日、
 *  XP 目标、今日净入账与锁定状态。字段名 reviewed_count 保留到 UI 数据源
 *  完成切换（本类型即切换后的消费面），语义 = 今日净 XP。 */
export interface CheckinStatus {
  /** 本地复习日 YYYYMMDD（02:00 日界线） */
  review_day: number;
  /** 每日 XP 目标（应用设置 learning.dailyXpGoal，缺省 40） */
  goal: number;
  /** 本复习日净入账 XP（负结算计入） */
  reviewed_count: number;
  /** 当前到期卡片数（课程 + 自定义） */
  due_count: number;
  /** 当日是否已锁定为完成（今日 XP 达标即锁定） */
  completed: boolean;
  /** 完成锁定时刻（UTC 毫秒），未完成时为 null */
  locked_at: number | null;
}

/** XP 读数（对齐后端 XpReading）：两把尺同源回放、互不换算——努力尺
 *  （今日 XP/目标）与进度尺（结算/总节点，现存且启用双筛）。纯投影。 */
export interface XpReading {
  /** 当前本地复习日 YYYYMMDD */
  review_day: number;
  /** 本复习日净入账 XP */
  today_xp: number;
  /** 每日 XP 目标 */
  goal: number;
  /** 今日 XP 是否已达标（实时判定，未含锁定语义） */
  goal_achieved: boolean;
  /** 进度尺分子：现存且启用课程内已结算节点数 */
  settled_nodes: number;
  /** 进度尺分母：现存且启用课程总节点数 */
  total_nodes: number;
  /** 未结算节点的剩余 est 分钟 */
  remaining_est: number;
  /** 按近 7 个学习日步伐的剩余天数估计；无步伐信号为 null；纯投影绝不回写 */
  eta_days: number | null;
}

/** XP 日历单日桶（学习日 02:00 翻转）：净入账，负账如实 */
export interface XpCalendarDay {
  /** 本地复习日 YYYYMMDD */
  review_day: number;
  /** 当日净 XP（可为负；无结算为 0） */
  xp: number;
}

/** XP 日历：恒 weeks×7 桶、末桶 = 今日、不过课程筛 */
export interface XpCalendar {
  /** 窗口周数（1..=53） */
  weeks: number;
  /** 分桶所用时区偏移（分钟，东正） */
  tz_offset: number;
  /** 恒 weeks×7 个日桶，旧 → 新，末桶 = 今日 */
  days: XpCalendarDay[];
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

/** R 软闸的一条建议：结构就绪但供给节点已遗忘——建议先复习供给节点 */
export interface GraphBlockedView {
  lesson_id: string;
  title: string;
  supplier_lesson_id: string;
  supplier_title: string;
  /** 供给节点代表预测回忆率（该节点全部复习卡的最小值） */
  r: number;
  due_count: number;
}

/** 图视图（挂在 CourseDetail.graph 下）：就绪集与水位。
 * 没有先修边、没有锁定态——发布即就绪。结构读侧投影走独立的
 * LearningStructureView（GET graph/structure）。 */
export interface LearningGraphView {
  goal: string;
  /** 下一步推荐学习的节点（就绪集 ≤10；被 R 软闸挡住的进 blocked） */
  recommended: string[];
  /** R 软闸建议（≤3 条，按供给节点回忆率升序） */
  blocked: GraphBlockedView[];
  /** 当前就绪存量与水位契约（补货目标 7 / 自动触发线 3） */
  ready_count: number;
  ready_target: number;
  ready_trigger: number;
  /** 是否有生成/生长运行进行中 */
  growth_running: boolean;
}

/** 结构读侧投影（GET /courses/{id}/graph/structure，always 200）：
 * structure = null 表示知识结构未生成（Missing 合法空状态，生成中态由
 * WS structure_* 事件表达）；投影运行时重算、永不落盘；严格课程内——
 * 只读本课 structure_json 快照，不 join 概念登记表。mounts = 挂载三态
 * 投影（j08），null = 从未跑过挂载回合 */
export interface LearningStructureView {
  course_id: string;
  structure: StructureView | null;
  terrain: TerrainView | null;
  /** 快照生成时刻（ms）；null = 未生成 */
  generated_at: number | null;
  mounts: StructureMountsView | null;
}

/** 挂载读侧三态投影（j08）：stale = 结构 hash 失配（挂载后结构又变过）；
 * states 单源派生 mounted / outside / not_evaluated；index = 块 slug →
 * 已挂节点清单；observations = 结构外/空块/超载块观察（修补输入源） */
export interface StructureMountsView {
  stale: boolean;
  entries: MountEntryView[];
  index: MountIndexEntryView[];
  states: NodeMountStateView[];
  observations: MountObservationView[];
  mounted_count: number;
  outside_count: number;
  not_evaluated_count: number;
}

/** 节点挂载三态：mounted | outside | not_evaluated */
export interface NodeMountStateView {
  node: string;
  state: 'mounted' | 'outside' | 'not_evaluated';
}

/** 一条挂载事实：blocks 数组（空 = 已评估、声明结构外）；悬挂 slug 的引用
 * 逐条标 dangling = true 降级，不使整体 Broken */
export interface MountEntryView {
  node: string;
  blocks: MountBlockRefView[];
  reason: string;
}

/** 单个块引用的解析结果 */
export interface MountBlockRefView {
  slug: string;
  block_name: string | null;
  dangling: boolean;
}

/** mount_index 单元：块 slug → 已挂节点清单（只含可解析块） */
export interface MountIndexEntryView {
  block: string;
  block_name: string | null;
  nodes: string[];
}

/** 挂载观察：outside-node | empty-block | overloaded-block */
export interface MountObservationView {
  kind: string;
  detail: string;
}

/** 挂载回合摘要（POST graph/structure/mount）：mounted | skipped */
export interface MountRoundSummary {
  status: 'mounted' | 'skipped';
  pending_nodes: number;
  mounted_nodes: number;
  outside_nodes: number;
  total_entries: number;
  stale: boolean;
  structure_hash: number;
}

/** 修补回合摘要（POST graph/structure/repair，j09）：applied | noop | skipped */
export interface StructureRepairSummary {
  status: 'applied' | 'noop' | 'skipped';
  old_hash: number;
  new_hash: number;
  /** 应用的 op 人面标签（依应用序） */
  applied: string[];
  /** 级联注记（改名跟改/拆块边退场/并块去重去自环） */
  cascade: string[];
  /** 结构已改 = 既有挂载待重评估 */
  mounts_stale: boolean;
  noop: boolean;
  skipped: boolean;
}

/** 结构产物读侧视图（预计算面：主干分组 / 块名集 / 矩阵格 / 依赖分层 /
 * 枢纽出入度；层号 = 依赖深度，不是排期） */
export interface StructureView {
  scalars: StructureScalarsView;
  domain: StructureDomainView;
  spines: SpineView[];
  blocks: BlockView[];
  edges: StructureEdgeView[];
  gaps: StructureGapView[];
  quality: StructureQualityView[];
  /** 主干分组（块表序保持） */
  groups: SpineGroupView[];
  /** 块名集（升序） */
  block_names: string[];
  /** 主干×阶段矩阵格 */
  matrix: MatrixRowView[];
  /** 枢纽 Top10（assumes + composes 出入度和降序） */
  hubs: HubView[];
}

export interface StructureScalarsView {
  goal: string;
  /** capability | coverage */
  goal_type: string;
  proficiency_statement: string;
  /** 理论 | 工程 | 历史 | 技能 */
  domain_type: string;
  form_reference: string;
  /** 名录 | 章节 */
  source_tier: string;
  generated_by: string;
  disclaimer: string;
}

export interface StructureDomainView {
  core_definition: string;
  includes: string[];
  excludes: string[];
  /** 引用前置地形的门口编号（G1..Gn），由生成侧对账 */
  prerequisite_doors: string[];
  audience: string;
  scenarios: string[];
  granularity: string;
}

export interface SpineView {
  name: string;
  definition: string;
  key_points: string[];
  instance: string;
  thin_spine: boolean | null;
  reason: string;
}

/** 块视图行：layer = assumes 前置链深度（L0 起算，层号不是排期）；
 * no_prereq = 本块不做任何 assumes 边之源；
 * door_refs = 门位标签（覆盖本块主题的门口编号，旧快照无键 = 空数组） */
export interface BlockView {
  name: string;
  slug: string;
  spine: string;
  /** 入门 | 进阶 | 高级 */
  stage: '入门' | '进阶' | '高级';
  note: string;
  layer: number;
  no_prereq: boolean;
  door_refs: string[];
}

export interface StructureEdgeView {
  source: string;
  target: string;
  /** assumes | composes | contrasts（assumes：source = 需求方、target = 前置） */
  edge_type: 'assumes' | 'composes' | 'contrasts';
  /** know | use | teach */
  tier: 'know' | 'use' | 'teach';
  note: string;
}

export interface StructureGapView {
  item: string;
  direction: string;
}

export interface StructureQualityView {
  check: string;
  /** met | unmet */
  status: string;
  reason: string;
}

export interface SpineGroupView {
  spine: string;
  block_names: string[];
}

export interface MatrixRowView {
  spine: string;
  entryway: string[];
  intermediate: string[];
  advanced: string[];
}

export interface HubView {
  name: string;
  in_degree: number;
  out_degree: number;
}

/** 前置地形读侧视图：门口列表 + 标量回执 */
export interface TerrainView {
  learner_basis_assumed: string;
  disclaimer: string;
  doors: TerrainDoorView[];
}

export interface TerrainDoorView {
  id: string;
  criterion: string;
  /** hard | self_check */
  kind: 'hard' | 'self_check';
  /** 块覆盖统计：结构中被该门口 door_refs 标注覆盖的块数（无标注 = 0） */
  covered_blocks: number;
}

/** 学习记录视图：批次时间线（倒序）——生长史即课程史 */
export interface GraphHistoryView {
  batches: GraphBatchView[];
}

/** 一个生长批次的出生档案：序号/状态/批注/时刻 + 节点行（含学习者进度）。
 * status 批行状态机：pending 生长进行中（或中断）、applied 已落库、failed 失败 */
export interface GraphBatchView {
  batch_id: string;
  seq: number;
  status: 'pending' | 'applied' | 'failed';
  note: string;
  created_at: number;
  nodes: GraphNodeHistoryView[];
}

export interface GraphNodeHistoryView {
  lesson_id: string;
  title: string;
  /** est：专注工作量尺寸锚（后端 DB 列保留旧名 estimated_minutes） */
  est: number;
  status: LessonStatus;
  completed_at: number | null;
}

/** 概念表行：登记表概念 + 本课程的教/假定引用 + 跨课程来源标注 */
export interface GraphConceptRowView {
  concept_id: string;
  canonical: string;
  /** 活跃别名（j04 结构化记录：名字 + 投影定义 + 出处；退役名不列） */
  aliases: GraphConceptAliasView[];
  definition: string;
  /** 本课程内教/假定该概念的节点（带学习者进度状态） */
  refs: GraphConceptRefView[];
  /** 其他还在教该概念的课程标题（跨课程就绪的来源可见性） */
  other_courses: string[];
}

/** 一条活跃别名（j04）：该名字的意义（缺席 = 与概念定义同义）与出处 */
export interface GraphConceptAliasView {
  name: string;
  definition: string | null;
  attestation: string | null;
}

export interface GraphConceptRefView {
  lesson_id: string;
  title: string;
  /** teaches | assumes */
  role: 'teaches' | 'assumes';
  /** know | apply | teach */
  tier: ConceptTier;
  /** 投影名（j04）：节点引用该概念时使用的名字；老行为 null（回退正名） */
  ref_name: string | null;
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
