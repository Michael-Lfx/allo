import type { LessonStatus } from './types';

/** 导入课程的示例 JSON，作为导入对话框的默认填充内容 */
export const EMPTY_PACK = `{
  "title": "Linear Algebra Foundations",
  "description": "A source-backed starter course",
  "domain": "mathematics",
  "version": 1,
  "concepts": [
    { "key": "vector", "title": "Vector", "prerequisites": [] }
  ],
  "modules": [
    {
      "title": "Foundations",
      "lessons": [
        {
          "title": "What is a vector?",
          "estimated_minutes": 10,
          "concepts": ["vector"],
          "activities": [
            {
              "kind": "true_false",
              "prompt": "A geometric vector has magnitude and direction.",
              "answer": true,
              "concepts": ["vector"]
            }
          ]
        }
      ]
    }
  ]
}`;

/** 课时状态 → Arco Tag 颜色 */
export const statusColors: Record<LessonStatus, string> = {
  not_started: 'gray',
  in_progress: 'blue',
  completed: 'green',
};

// Sentinel value for the review-queue course filter that selects
// learner-authored questions belonging to no course at all.
export const ORPHAN_COURSE_FILTER = '__orphan__';

/** 题目表格中可切换显隐的列 */
export const QUESTION_SELECTABLE_COLUMNS = ['source', 'state', 'due_at', 'tags'];
export const QUESTION_COLUMNS_STORAGE_KEY = 'learning.questionTableColumns';
export const REVIEW_FILTERS_STORAGE_KEY = 'learning.reviewFilters';
export const REVIEW_BANNER_EXPANDED_KEY = 'learning.reviewBannerExpanded';
