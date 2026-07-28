import { httpRequest } from '@/common/adapter/httpBridge';
import type {
  AttemptResult,
  CourseDetail,
  CourseSummary,
  DiagnosticPlan,
  DueReview,
  GenerateCourseRequest,
  LessonStatus,
  ReviewRating,
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
  rateReview: (id: string, rating: ReviewRating) =>
    httpRequest<void>('POST', `${BASE}/reviews/${encodeURIComponent(id)}/rate`, { rating }),
};
