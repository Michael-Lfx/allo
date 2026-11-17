/** 导入课程的示例 JSON，作为导入对话框的默认填充内容 */
export const EMPTY_PACK = `{
  "title": "Linear Algebra Foundations",
  "description": "A source-backed starter course",
  "domain": "mathematics",
  "version": 1,
  "modules": [
    {
      "title": "Foundations",
      "lessons": [
        {
          "title": "What is a vector?",
          "est": 10,
          "activities": [
            {
              "kind": "true_false",
              "prompt": "A geometric vector has magnitude and direction.",
              "answer": true
            }
          ]
        }
      ]
    }
  ]
}`;

// Sentinel value for the review-queue course filter that selects
// learner-authored questions belonging to no course at all.
export const ORPHAN_COURSE_FILTER = '__orphan__';

/** 题目表格中可切换显隐的列 */
export const QUESTION_SELECTABLE_COLUMNS = ['source', 'state', 'due_at', 'tags'];
export const QUESTION_COLUMNS_STORAGE_KEY = 'learning.questionTableColumns';
export const REVIEW_FILTERS_STORAGE_KEY = 'learning.reviewFilters';
export const REVIEW_BANNER_EXPANDED_KEY = 'learning.reviewBannerExpanded';
/** 结构图自动呈现水位（c5）：某课的结构图被自动呈现过一次的持久标记。值 =
 * 结构 generated_at（ms）；重生成后时间戳更新 → 再次自动呈现一次。 */
export const STRUCTURE_PRESENTED_KEY = 'learning.structurePresented';
/** 图工作区头部折叠（f3 Q5）：per-course 折叠标记。值 = { [courseId]: true }
 * （只记折叠；缺席 = 展开——默认展开语义）。 */
export const GRAPH_HEADER_COLLAPSED_KEY = 'learning.graph.headerCollapsed';

/** 图工作区头部滚动自动折叠阈值（a1，px）：学习 Tab 正文滚动深度 ≥ 该值时
 * 临时折叠头部（让位正文版面）。临时层不落盘（滚动深度是视口语境不是偏好），
 * 常量可调参（真机目验欠账：跟手不闪烁、临界不抖动）。 */
export const HEADER_COLLAPSE_SCROLL_COLLAPSE_PX = 160;
/** 图工作区头部滚动自动展开阈值（a1，px）= 折叠阈值的 H/2 迟滞：滚动深度
 * 低于该值恢复展开并复位用户否决臂（迟滞带 [H/2, H) 内维持现态，防临界抖动）。 */
export const HEADER_COLLAPSE_SCROLL_EXPAND_PX = HEADER_COLLAPSE_SCROLL_COLLAPSE_PX / 2;

/**
 * 归档确认的"算法数据达标"阈值：复习次数不少于该值，且记忆稳定性不低于
 * 该天数（天）。任一不满足即视为"未达标"，归档前需二次确认——归档等于
 * 放弃一段仍在形成的记忆。稳定性阈值定得保守，因为 FSRS 指标波动大。
 */
export const ARCHIVE_STABLE_REVIEW_COUNT = 2;
export const ARCHIVE_STABLE_DAYS = 30;

/** 学习页轮询节拍共享常量（S6）：图课程流结构徽章轮询（structurePillWatch）
 * 与课时生成耗时计时拍（LessonStudy genElapsed interval）同口径——5s 一拍，
 * 终态即停。定义在共享 constants，两处消费同名常量（魔数不再两处手写）。 */
export const STRUCTURE_PILL_POLL_MS = 5_000;
