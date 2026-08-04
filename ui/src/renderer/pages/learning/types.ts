export type ActivityKind = 'single_choice' | 'true_false' | 'reflection';
export type LessonStatus = 'not_started' | 'in_progress' | 'completed';
export type ReviewRating = 'again' | 'hard' | 'good' | 'easy';

export interface GenerateCourseRequest {
  knowledge_base_id: string;
  domain?: string;
  provider_id?: string;
  model?: string;
  module_count?: number;
  lessons_per_module?: number;
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
  position: number;
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

export interface ReviewQuestion {
  activity_id: string;
  kind: ActivityKind;
  prompt: string;
  options: string[];
}

export interface DueReview {
  id: string;
  enrollment_id: string;
  course_id: string;
  course_title: string;
  module_title: string;
  lesson_title: string;
  concept_id: string;
  concept_title: string;
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

export interface ReviewAnswerResult {
  correct: boolean;
  feedback: string;
  correct_answer: unknown | null;
  rated: ReviewResult | null;
}
