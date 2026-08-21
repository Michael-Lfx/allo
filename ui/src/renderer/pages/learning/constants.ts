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

/**
 * 归档确认的"算法数据达标"阈值：复习次数不少于该值，且记忆稳定性不低于
 * 该天数（天）。任一不满足即视为"未达标"，归档前需二次确认——归档等于
 * 放弃一段仍在形成的记忆。稳定性阈值定得保守，因为 FSRS 指标波动大。
 */
export const ARCHIVE_STABLE_REVIEW_COUNT = 2;
export const ARCHIVE_STABLE_DAYS = 30;
