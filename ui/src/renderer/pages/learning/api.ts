import { httpRequest } from '@/common/adapter/httpBridge';
import type {
  AttemptResult,
  CourseDetail,
  CourseSummary,
  DiagnosticPlan,
  DueReview,
  GenerateCourseRequest,
  LessonStatus,
  QuestionEntry,
  ReviewAnswerResult,
  ReviewRating,
  UpdateQuestionRequest,
} from './types';

const BASE = '/api/learning';

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
  listDueReviews: (limit = 30) =>
    httpRequest<DueReview[]>('GET', `${BASE}/reviews/due?limit=${limit}`),
  answerReview: (id: string, response: unknown, forgot = false) =>
    httpRequest<ReviewAnswerResult>('POST', `${BASE}/reviews/${encodeURIComponent(id)}/answer`, {
      response,
      forgot,
    }),
  rateReview: (id: string, rating: ReviewRating) =>
    httpRequest<void>('POST', `${BASE}/reviews/${encodeURIComponent(id)}/rate`, { rating }),
  skipReview: (id: string) =>
    httpRequest<void>('POST', `${BASE}/reviews/${encodeURIComponent(id)}/skip`),
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
  updateQuestion: (activityId: string, request: UpdateQuestionRequest) =>
    httpRequest<void>('PUT', `${BASE}/questions/${encodeURIComponent(activityId)}`, request),
  deleteCourse: (id: string, deleteReviews: boolean) =>
    httpRequest<void>('DELETE', `${BASE}/courses/${encodeURIComponent(id)}`, {
      delete_reviews: deleteReviews,
    }),
};
