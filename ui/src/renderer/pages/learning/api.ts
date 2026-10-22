import { httpRequest } from '@/common/adapter/httpBridge';
import type {
  AttemptResult,
  CalendarStats,
  CheckinStatus,
  CourseDetail,
  CourseSummary,
  CreateCustomQuestionRequest,
  CreateLessonActivityRequest,
  DueReview,
  GenerateCourseRequest,
  GenerateLessonActivityRequest,
  GenerateLessonRequest,
  GraphConceptRowView,
  GraphHistoryView,
  LearningStructureView,
  MemoryHealthStats,
  GeneratedLessonActivity,
  LearningGraphGenerationStatus,
  Lesson,
  LessonStatus,
  QuestionEntry,
  ReviewAnswerResult,
  ReviewRating,
  ReviewResult,
  ReviewSource,
  SetTagsRequest,
  SubmitAttemptRequest,
  UpdateQuestionRequest,
  UpdateSectionBodyRequest,
  MountRoundSummary,
  StructureRepairSummary,
  XpCalendar,
  XpReading,
} from './types';

const BASE = '/api/learning';

const reviewBase = (source: ReviewSource, id: string) =>
  source === 'custom'
    ? `${BASE}/custom-questions/${encodeURIComponent(id)}`
    : `${BASE}/reviews/${encodeURIComponent(id)}`;

export const learningApi = {
  listCourses: () => httpRequest<CourseSummary[]>('GET', `${BASE}/courses`),
  importCourse: (pack: unknown) => httpRequest<CourseDetail>('POST', `${BASE}/courses`, pack),
  // 同步创建：课程落库即返回；知识结构生成与首生长在后台执行（进度经
  // WS 的 `learning.course-generation` 推送，状态/取消用下方这对端点）。
  generateCourse: (request: GenerateCourseRequest) =>
    httpRequest<CourseDetail>('POST', `${BASE}/courses/generate`, request),
  // t5 成本门估算：提交前对知识库流做一次规则分诊（零 LLM），读数含
  // 文件数/批数/调用/耗时与跳过摘要；needs_confirm 由后端按阈值判定。
  estimateGeneration: (request: GenerateCourseRequest) =>
    httpRequest<import('./types').CourseGenerationEstimateView>(
      'POST',
      `${BASE}/courses/generate/estimate`,
      request
    ),
  // 学习图生成/生长状态/取消：后台运行对外可发现、可取消。
  generationStatus: () =>
    httpRequest<LearningGraphGenerationStatus>(
      'GET',
      `${BASE}/courses/generate/status`
    ),
  cancelGeneration: () =>
    httpRequest<{ cancelled: boolean }>(
      'POST',
      `${BASE}/courses/generate/cancel`,
      {}
    ),
  // 手动触发一次生长（就绪补到 7；已在生长中报 409）
  growGraph: (courseId: string) =>
    httpRequest<{ kicked: boolean }>(
      'POST',
      `${BASE}/courses/${encodeURIComponent(courseId)}/graph/grow`,
      {}
    ),
  // 学习记录：批次时间线（倒序）
  graphHistory: (courseId: string) =>
    httpRequest<GraphHistoryView>(
      'GET',
      `${BASE}/courses/${encodeURIComponent(courseId)}/graph/history`
    ),
  // 概念表：本课程涉及的概念（教/假定 × 档位 × 节点）+ 跨课程来源
  graphConcepts: (courseId: string) =>
    httpRequest<GraphConceptRowView[]>(
      'GET',
      `${BASE}/courses/${encodeURIComponent(courseId)}/graph/concepts`
    ),
  // 结构读侧投影（always 200；structure = null 表示未生成）：投影运行时
  // 重算、永不落盘；前端结构图以 WS structure_* 事件触发重取本接口
  graphStructure: (courseId: string) =>
    httpRequest<LearningStructureView>(
      'GET',
      `${BASE}/courses/${encodeURIComponent(courseId)}/graph/structure`
    ),
  // 重跑知识结构生成（换参考地图不换课）；进度走 WS structure_* 事件
  regenerateGraphStructure: (
    courseId: string,
    request: { provider_id?: string; model?: string } = {}
  ) =>
    httpRequest<{ kicked: boolean }>(
      'POST',
      `${BASE}/courses/${encodeURIComponent(courseId)}/graph/structure/regenerate`,
      request
    ),
  // 结构挂载回合（j08）：把待挂集节点挂到结构块（导航事实）；同步返回
  // 回合摘要（mounted | skipped）
  mountStructure: (
    courseId: string,
    request: { provider_id?: string; model?: string } = {}
  ) =>
    httpRequest<MountRoundSummary>(
      'POST',
      `${BASE}/courses/${encodeURIComponent(courseId)}/graph/structure/mount`,
      request
    ),
  // 结构修补回合（j09）：按留痕与观察输出七型修补意图；皆空 = skipped，
  // ops 空 = noop；同步返回回合摘要
  repairStructure: (
    courseId: string,
    request: { provider_id?: string; model?: string } = {}
  ) =>
    httpRequest<StructureRepairSummary>(
      'POST',
      `${BASE}/courses/${encodeURIComponent(courseId)}/graph/structure/repair`,
      request
    ),
  getCourse: (id: string) =>
    httpRequest<CourseDetail>('GET', `${BASE}/courses/${encodeURIComponent(id)}`),
  enroll: (id: string) =>
    httpRequest<CourseDetail>('POST', `${BASE}/courses/${encodeURIComponent(id)}/enroll`),
  getLesson: (id: string) =>
    httpRequest<Lesson>('GET', `${BASE}/lessons/${encodeURIComponent(id)}`),
  updateLessonProgress: (id: string, status: LessonStatus) =>
    httpRequest<void>('POST', `${BASE}/lessons/${encodeURIComponent(id)}/progress`, { status }),
  generateLesson: (id: string, request: GenerateLessonRequest = {}) =>
    httpRequest<Lesson>('POST', `${BASE}/lessons/${encodeURIComponent(id)}/generate`, request),
  // 单节重写（ADR-0003）：确定性单节管线，返回重写后的最新课时详情
  //（其余节与题目不动）；旧课时（无节清单）返回 400。feedback 为可选
  // 学习建议（ADR-0007），为空即同分布重生成。
  rewriteLessonSection: (id: string, sectionKey: string, request: GenerateLessonRequest = {}) =>
    httpRequest<Lesson>(
      'POST',
      `${BASE}/lessons/${encodeURIComponent(id)}/sections/${encodeURIComponent(sectionKey)}/rewrite`,
      request
    ),
  // 手动编辑节正文（ADR-0007）：仅覆盖 body_md，返回编辑后的最新课时详情
  updateLessonSectionBody: (id: string, sectionKey: string, request: UpdateSectionBodyRequest) =>
    httpRequest<Lesson>(
      'PUT',
      `${BASE}/lessons/${encodeURIComponent(id)}/sections/${encodeURIComponent(sectionKey)}/body`,
      request
    ),
  createLessonActivity: (lessonId: string, request: CreateLessonActivityRequest) =>
    httpRequest<Lesson>(
      'POST',
      `${BASE}/lessons/${encodeURIComponent(lessonId)}/activities`,
      request
    ),
  generateLessonActivity: (lessonId: string, request: GenerateLessonActivityRequest) =>
    httpRequest<GeneratedLessonActivity>(
      'POST',
      `${BASE}/lessons/${encodeURIComponent(lessonId)}/activities/generate`,
      request
    ),
  submitAttempt: (id: string, request: SubmitAttemptRequest) =>
    httpRequest<AttemptResult>('POST', `${BASE}/activities/${encodeURIComponent(id)}/attempts`, request),
  listDueReviews: (
    limit = 30,
    courseId?: string | string[],
    options?: { dueOnly?: boolean; orphan?: boolean; tags?: string[] }
  ) => {
    const query = new URLSearchParams({ limit: String(limit) });
    const courseIds = courseId === undefined ? [] : Array.isArray(courseId) ? courseId : [courseId];
    for (const id of courseIds) query.append('course_id', id);
    if (options?.dueOnly) query.set('due_only', 'true');
    if (options?.orphan) query.set('orphan', 'true');
    for (const tag of options?.tags ?? []) query.append('tag', tag);
    return httpRequest<DueReview[]>('GET', `${BASE}/reviews/due?${query.toString()}`);
  },
  listTags: () => httpRequest<string[]>('GET', `${BASE}/tags`),
  checkinToday: () => httpRequest<CheckinStatus>('GET', `${BASE}/checkins/today`),
  /** XP 读数（两把尺同源回放：今日 XP/目标 + 结算进度尺，ETA 纯投影） */
  getXpReading: () => httpRequest<XpReading>('GET', `${BASE}/xp/reading`),
  /** XP 日历：恒 weeks×7 桶含零桶、末桶 = 今日（缺省 12 周） */
  getXpCalendar: (weeks = 12) =>
    httpRequest<XpCalendar>('GET', `${BASE}/xp/calendar?weeks=${weeks}`),
  getMemoryStats: (tzOffset: number) =>
    httpRequest<MemoryHealthStats>('GET', `${BASE}/stats/memory?tz_offset=${tzOffset}`),
  getCalendarStats: (year: number, month: number | undefined, tzOffset: number) =>
    httpRequest<CalendarStats>(
      'GET',
      `${BASE}/stats/calendar?tz_offset=${tzOffset}&year=${year}${month ? `&month=${month}` : ''}`
    ),
  setCourseTags: (id: string, request: SetTagsRequest) =>
    httpRequest<string[]>('PUT', `${BASE}/courses/${encodeURIComponent(id)}/tags`, request),
  setQuestionTags: (
    entry: Pick<QuestionEntry, 'source' | 'question_id'>,
    tags: string[]
  ) =>
    entry.source === 'custom'
      ? httpRequest<string[]>(
          'PUT',
          `${BASE}/custom-questions/${encodeURIComponent(entry.question_id)}/tags`,
          { tags }
        )
      : httpRequest<string[]>(
          'PUT',
          `${BASE}/questions/${encodeURIComponent(entry.question_id)}/tags`,
          { tags }
        ),
  answerReview: (
    source: ReviewSource,
    id: string,
    response: unknown,
    forgot = false,
    elapsedMs?: number
  ) =>
    httpRequest<ReviewAnswerResult>('POST', `${reviewBase(source, id)}/answer`, {
      response,
      forgot,
      // 题面展示到提交的墙钟耗时，供乱猜判定等后续启发式使用
      ...(elapsedMs === undefined ? {} : { elapsed_ms: elapsedMs }),
    }),
  rateReview: (source: ReviewSource, id: string, rating: ReviewRating) =>
    httpRequest<ReviewResult>('POST', `${reviewBase(source, id)}/rate`, { rating }),
  skipReview: (source: ReviewSource, id: string) =>
    httpRequest<ReviewResult>('POST', `${reviewBase(source, id)}/skip`),
  deleteReviewItem: (id: string) =>
    httpRequest<void>('DELETE', `${BASE}/reviews/${encodeURIComponent(id)}`),
  archiveReview: (id: string) =>
    httpRequest<void>('POST', `${BASE}/reviews/${encodeURIComponent(id)}/archive`),
  unarchiveReview: (id: string) =>
    httpRequest<void>('POST', `${BASE}/reviews/${encodeURIComponent(id)}/unarchive`),
  /** 标记课程复习卡为待编辑，note 选填，用于找回编辑思路 */
  markReviewEditPending: (id: string, note: string) =>
    httpRequest<void>('POST', `${BASE}/reviews/${encodeURIComponent(id)}/mark-edit`, { note }),
  /** 课程复习卡完整信息（含答案），供刷卡界面编辑对话框加载 */
  getReviewQuestion: (id: string) =>
    httpRequest<QuestionEntry>('GET', `${BASE}/reviews/${encodeURIComponent(id)}`),
  archiveCustomQuestion: (id: string) =>
    httpRequest<void>('POST', `${BASE}/custom-questions/${encodeURIComponent(id)}/archive`),
  unarchiveCustomQuestion: (id: string) =>
    httpRequest<void>('POST', `${BASE}/custom-questions/${encodeURIComponent(id)}/unarchive`),
  /** 标记自建题为待编辑，note 选填，用于找回编辑思路 */
  markCustomEditPending: (id: string, note: string) =>
    httpRequest<void>('POST', `${BASE}/custom-questions/${encodeURIComponent(id)}/mark-edit`, {
      note,
    }),
  /** 自定义问题完整信息（含答案），供刷卡界面编辑对话框加载 */
  getCustomQuestion: (id: string) =>
    httpRequest<QuestionEntry>('GET', `${BASE}/custom-questions/${encodeURIComponent(id)}`),
  listQuestions: (params: { course_id?: string; state?: string; search?: string }) => {
    const query = new URLSearchParams();
    if (params.course_id) query.set('course_id', params.course_id);
    if (params.state) query.set('state', params.state);
    if (params.search) query.set('search', params.search);
    const suffix = query.size > 0 ? `?${query.toString()}` : '';
    return httpRequest<QuestionEntry[]>('GET', `${BASE}/questions${suffix}`);
  },
  updateQuestion: (entry: Pick<QuestionEntry, 'source' | 'question_id'>, request: UpdateQuestionRequest) =>
    entry.source === 'custom'
      ? httpRequest<void>(
          'PUT',
          `${BASE}/custom-questions/${encodeURIComponent(entry.question_id)}`,
          request
        )
      : httpRequest<void>(
          'PUT',
          `${BASE}/questions/${encodeURIComponent(entry.question_id)}`,
          request
        ),
  createCustomQuestion: (request: CreateCustomQuestionRequest) =>
    httpRequest<string>('POST', `${BASE}/custom-questions`, request),
  deleteCustomQuestion: (id: string) =>
    httpRequest<void>('DELETE', `${BASE}/custom-questions/${encodeURIComponent(id)}`),
  /** 删除课程（三轮 g3：账本永远保留，无连带删卡选项；内容移除、学习记录保留） */
  deleteCourse: (id: string) =>
    httpRequest<void>('DELETE', `${BASE}/courses/${encodeURIComponent(id)}`),
  /** 课程改名（只动显示名；目标/快照/学习数据不动） */
  renameCourse: (id: string, title: string) =>
    httpRequest<CourseSummary>('PATCH', `${BASE}/courses/${encodeURIComponent(id)}`, { title }),
  /** 归档课程（enabled 翻转，数据零删零改，可恢复） */
  archiveCourse: (id: string) =>
    httpRequest<CourseSummary>('POST', `${BASE}/courses/${encodeURIComponent(id)}/archive`),
  /** 恢复归档课程 */
  restoreCourse: (id: string) =>
    httpRequest<CourseSummary>('POST', `${BASE}/courses/${encodeURIComponent(id)}/restore`),
};
