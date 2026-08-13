import { httpRequest } from '@/common/adapter/httpBridge';
import type {
  AttemptResult,
  ConceptRef,
  CourseDetail,
  CourseSummary,
  CreateCustomQuestionRequest,
  DiagnosticPlan,
  DueReview,
  GenerateCourseRequest,
  LessonStatus,
  QuestionEntry,
  ReviewAnswerResult,
  ReviewRating,
  ReviewResult,
  ReviewSource,
  SetTagsRequest,
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
    httpRequest<CourseDetail>('POST', `${BASE}/courses/generate`, request),
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
  submitAttempt: (id: string, response: unknown) =>
    httpRequest<AttemptResult>('POST', `${BASE}/activities/${encodeURIComponent(id)}/attempts`, {
      response,
    }),
  listDueReviews: (limit = 30, courseId?: string) => {
    const query = new URLSearchParams({ limit: String(limit) });
    if (courseId) query.set('course_id', courseId);
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
