import { httpRequest } from '@/common/adapter/httpBridge';
import type {
  AttemptResult,
  ConceptRef,
  CourseDetail,
  CourseJobView,
  CourseSummary,
  CreateCustomQuestionRequest,
  DiagnosticPlan,
  DueReview,
  GenerateCourseRequest,
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
    activityId?: string | null
  ) =>
    httpRequest<ReviewAnswerResult>('POST', `${reviewBase(source, id)}/answer`, {
      response,
      forgot,
      // Course cards identify the exact question of the expanded queue.
      ...(activityId ? { activity_id: activityId } : {}),
    }),
  rateReview: (source: ReviewSource, id: string, rating: ReviewRating) =>
    httpRequest<ReviewResult>('POST', `${reviewBase(source, id)}/rate`, { rating }),
  skipReview: (source: ReviewSource, id: string) =>
    httpRequest<ReviewResult>('POST', `${reviewBase(source, id)}/skip`),
  deleteReviewItem: (id: string) =>
    httpRequest<void>('DELETE', `${BASE}/reviews/${encodeURIComponent(id)}`),
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
