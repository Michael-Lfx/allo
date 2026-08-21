import { httpRequest } from '@/common/adapter/httpBridge';
import type {
  AttemptResult,
  CalendarStats,
  CheckinStatus,
  ConceptRef,
  CourseDetail,
  CourseJobView,
  CourseSummary,
  CreateCustomQuestionRequest,
  CreateLessonActivityRequest,
  DiagnosticPlan,
  DueReview,
  GenerateCourseRequest,
  GenerateLessonActivityRequest,
  GenerateLessonRequest,
  GeneratedLessonActivity,
  Lesson,
  LessonStatus,
  QuestionEntry,
  RetryCourseJobRequest,
  ReviewAnswerResult,
  ReviewRating,
  ReviewResult,
  ReviewSource,
  SetTagsRequest,
  SubmitAttemptRequest,
  UpdateQuestionRequest,
} from './types';

const BASE = '/api/learning';

const reviewBase = (source: ReviewSource, id: string) =>
  source === 'custom'
    ? `${BASE}/custom-questions/${encodeURIComponent(id)}`
    : `${BASE}/reviews/${encodeURIComponent(id)}`;

export const learningApi = {
  listCourses: () => httpRequest<CourseSummary[]>('GET', `${BASE}/courses`),
  importCourse: (pack: unknown) => httpRequest<CourseDetail>('POST', `${BASE}/courses`, pack),
  generateCourse: (request: GenerateCourseRequest) =>
    httpRequest<CourseJobView>('POST', `${BASE}/courses/generate`, request),
  listCourseJobs: () => httpRequest<CourseJobView[]>('GET', `${BASE}/course-jobs`),
  getCourseJob: (id: string) =>
    httpRequest<CourseJobView>('GET', `${BASE}/course-jobs/${encodeURIComponent(id)}`),
  cancelCourseJob: (id: string) =>
    httpRequest<CourseJobView>('POST', `${BASE}/course-jobs/${encodeURIComponent(id)}/cancel`),
  resumeCourseJob: (id: string) =>
    httpRequest<CourseJobView>('POST', `${BASE}/course-jobs/${encodeURIComponent(id)}/resume`),
  retryCourseJob: (id: string, request: RetryCourseJobRequest) =>
    httpRequest<CourseJobView>('POST', `${BASE}/course-jobs/${encodeURIComponent(id)}/retry`, request),
  deleteCourseJob: (id: string) =>
    httpRequest<void>('DELETE', `${BASE}/course-jobs/${encodeURIComponent(id)}`),
  getCourse: (id: string) =>
    httpRequest<CourseDetail>('GET', `${BASE}/courses/${encodeURIComponent(id)}`),
  enroll: (id: string) =>
    httpRequest<CourseDetail>('POST', `${BASE}/courses/${encodeURIComponent(id)}/enroll`),
  getDiagnostic: (id: string, limit = 10) =>
    httpRequest<DiagnosticPlan>(
      'GET',
      `${BASE}/courses/${encodeURIComponent(id)}/diagnostic?limit=${limit}`
    ),
  updateLessonProgress: (id: string, status: LessonStatus) =>
    httpRequest<void>('POST', `${BASE}/lessons/${encodeURIComponent(id)}/progress`, { status }),
  generateLesson: (id: string, request: GenerateLessonRequest = {}) =>
    httpRequest<Lesson>('POST', `${BASE}/lessons/${encodeURIComponent(id)}/generate`, request),
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
  answerReview: (source: ReviewSource, id: string, response: unknown, forgot = false) =>
    httpRequest<ReviewAnswerResult>('POST', `${reviewBase(source, id)}/answer`, {
      response,
      forgot,
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
  listConceptRefs: () => httpRequest<ConceptRef[]>('GET', `${BASE}/concepts`),
  deleteCourse: (id: string, deleteReviews: boolean) =>
    httpRequest<void>('DELETE', `${BASE}/courses/${encodeURIComponent(id)}`, {
      delete_reviews: deleteReviews,
    }),
};
