export type ActivityKind = 'single_choice' | 'true_false' | 'reflection' | 'fill_in_blank';
export type LessonStatus = 'not_started' | 'in_progress' | 'completed';
export type ReviewRating = 'again' | 'hard' | 'good' | 'easy';
export type ReviewSource = 'course' | 'custom';
export type QuestionState = 'unlearned' | 'new' | 'due' | 'scheduled';

export interface GenerateCourseRequest {
  knowledge_base_id: string;
  domain?: string;
  provider_id?: string;
  model?: string;
  module_count?: number;
  lessons_per_module?: number;
  /** 'full' 一次性生成全部课时；'on_demand' 先出大纲，学习时按需生成课时 */
  mode?: 'full' | 'on_demand';
}

/** 按需生成单个课时内容时可选的模型偏好；两个字段同时传或不传 */
export interface GenerateLessonRequest {
  provider_id?: string;
  model?: string;
}

/** 重试课程生成任务时可选的模型偏好；两个字段同时传或不传 */
export interface RetryCourseJobRequest {
  provider_id?: string;
  model?: string;
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
}

export interface Activity {
  id: string;
  kind: ActivityKind;
  prompt: string;
  options: string[];
  position: number;
  concepts: string[];
}

export interface DiagnosticItem {
  lesson_id: string;
  lesson_title: string;
  activity: Activity;
}

export interface DiagnosticPlan {
  course_id: string;
  total_concepts: number;
  items: DiagnosticItem[];
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
  concepts: string[];
  activities: Activity[];
}

export interface LearningModule {
  id: string;
  title: string;
  description: string;
  position: number;
  lessons: Lesson[];
}

export interface Concept {
  id: string;
  key: string;
  title: string;
  description: string;
  prerequisites: string[];
  mastery: number | null;
}

export interface CourseDetail {
  course: CourseSummary;
  enrollment_id: string | null;
  modules: LearningModule[];
  concepts: Concept[];
  next_lesson_id: string | null;
  due_review_count: number;
}

export interface AttemptResult {
  id: string;
  score: number;
  passed: boolean;
  feedback: string;
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
}

export interface DueReview {
  id: string;
  source: ReviewSource;
  enrollment_id: string | null;
  course_id: string | null;
  course_title: string | null;
  module_title: string | null;
  lesson_title: string | null;
  concept_id: string | null;
  concept_title: string | null;
  question: ReviewQuestion;
  due_at: number;
  stability_days: number;
  difficulty: number;
  review_count: number;
  lapse_count: number;
}

export interface ReviewResult {
  id: string;
  due_at: number;
  stability_days: number;
  difficulty: number;
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
}

export interface QuestionEntry {
  source: ReviewSource;
  question_id: string;
  review_item_id: string | null;
  state: QuestionState;
  course_id: string | null;
  course_title: string | null;
  concept_id: string | null;
  concept_title: string | null;
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
  concept_id?: string | null;
  /** 填空题的近义干扰项（可选，仅填空题型使用） */
  distractors?: string[];
}

/** 手动向课时追加练习（4 种题型全支持）；concept_ids 为空时后端绑定课时全部概念 */
export interface CreateLessonActivityRequest {
  kind: ActivityKind;
  prompt: string;
  options?: string[];
  answer: unknown;
  explanation?: string;
  /** 填空题的近义干扰项（可选，仅填空题型使用） */
  distractors?: string[];
  concept_ids?: string[];
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
  /** 建议绑定的概念（默认课时概念） */
  concept_ids: string[];
}

export interface ConceptRef {
  concept_id: string;
  title: string;
  course_title: string | null;
}

export interface SetTagsRequest {
  tags: string[];
  apply_to_children?: boolean;
}

export type CourseJobSource = 'http' | 'agent';

export type CourseJobStatus =
  | 'queued'
  | 'sampling'
  | 'blueprint'
  | 'lessons'
  | 'importing'
  | 'completed'
  | 'failed'
  | 'cancelled'
  | 'interrupted';

/** 持久化课程生成任务的公开投影（对应后端 CourseJobView） */
export interface CourseJobView {
  job_id: string;
  source: CourseJobSource;
  status: CourseJobStatus;
  /** 1 起始的模块索引；蓝图完成前为 0 */
  current_module: number;
  /** 已完成课时数（0..=total_lessons） */
  current_lesson: number;
  total_lessons: number;
  error: string | null;
  course_id: string | null;
  /** 任务对应的知识库名称（库已被删除时为 null） */
  knowledge_base_name: string | null;
  /** 用户填写的课程领域（请求快照中，未填时为 null） */
  domain: string | null;
  created_at: number;
  updated_at: number;
}
