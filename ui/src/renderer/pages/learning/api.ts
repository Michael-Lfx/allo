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
  EndpointInput,
  EndpointUpdateInput,
  GenerateCourseRequest,
  GenerateLessonActivityRequest,
  GenerateLessonRequest,
  GraphConceptRowView,
  GraphEndpointView,
  GraphHistoryView,
  MemoryHealthStats,
  GeneratedLessonActivity,
  LearningGraphGenerationStatus,
  ProposedEndpointView,
  ProposeEndpointsRequest,
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
} from './types';

const BASE = '/api/learning';

const reviewBase = (source: ReviewSource, id: string) =>
  source === 'custom'
    ? `${BASE}/custom-questions/${encodeURIComponent(id)}`
    : `${BASE}/reviews/${encodeURIComponent(id)}`;

export const learningApi = {
  listCourses: () => httpRequest<CourseSummary[]>('GET', `${BASE}/courses`),
  importCourse: (pack: unknown) => httpRequest<CourseDetail>('POST', `${BASE}/courses`, pack),
  // 同步创建：目标解析与终点提议在请求内完成；首生长在后台执行（进度经
  // WS 的 `learning.course-generation` 推送，状态/取消用下方这对端点）。
  generateCourse: (request: GenerateCourseRequest) =>
    httpRequest<CourseDetail>('POST', `${BASE}/courses/generate`, request),
  // 为学习目标提议终点锚（建课向导第二步；提议失败返回空列表，引导手填）
  proposeGraphEndpoints: (request: ProposeEndpointsRequest) =>
    httpRequest<ProposedEndpointView[]>(
      'POST',
      `${BASE}/graph/propose-endpoints`,
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
  // 手动重画罗盘（UI 罗盘卡的"重画"入口）
  redrawGraphCompass: (courseId: string) =>
    httpRequest<void>(
      'POST',
      `${BASE}/courses/${encodeURIComponent(courseId)}/graph/compass/redraw`,
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
  // 终点锚 CRUD（任何变更后端自动重画罗盘）
  addGraphEndpoint: (courseId: string, request: EndpointInput) =>
    httpRequest<GraphEndpointView>(
      'POST',
      `${BASE}/courses/${encodeURIComponent(courseId)}/graph/endpoints`,
      request
    ),
  updateGraphEndpoint: (courseId: string, endpointId: string, request: EndpointUpdateInput) =>
    httpRequest<void>(
      'PUT',
      `${BASE}/courses/${encodeURIComponent(courseId)}/graph/endpoints/${encodeURIComponent(endpointId)}`,
      request
    ),
  deleteGraphEndpoint: (courseId: string, endpointId: string) =>
    httpRequest<void>(
      'DELETE',
      `${BASE}/courses/${encodeURIComponent(courseId)}/graph/endpoints/${encodeURIComponent(endpointId)}`
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
  deleteCourse: (id: string, deleteReviews: boolean) =>
    httpRequest<void>('DELETE', `${BASE}/courses/${encodeURIComponent(id)}`, {
      delete_reviews: deleteReviews,
    }),
};
