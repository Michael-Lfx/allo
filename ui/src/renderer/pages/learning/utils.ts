import { useTranslation } from 'react-i18next';
import {
  QUESTION_COLUMNS_STORAGE_KEY,
  QUESTION_SELECTABLE_COLUMNS,
  REVIEW_FILTERS_STORAGE_KEY,
} from './constants';
import type { LessonStatus, QuestionEntry } from './types';

export type Translate = ReturnType<typeof useTranslation>['t'];

export function statusLabel(status: LessonStatus, t: Translate): string {
  const labels: Record<LessonStatus, string> = {
    not_started: t('learning.notStarted'),
    in_progress: t('learning.inProgress'),
    completed: t('learning.completed'),
  };
  return labels[status];
}

export function formatReviewTime(value: number | null): string {
  return value === null ? '—' : new Date(value).toLocaleString();
}

export function questionStateMeta(
  entry: QuestionEntry,
  t: (key: string) => string
): { label: string; color: string } {
  if (entry.state === 'unlearned') {
    return { label: t('learning.questionStateUnlearned'), color: 'gray' };
  }
  if (entry.state === 'new') {
    return { label: t('learning.questionStateNew'), color: 'blue' };
  }
  if (entry.state === 'due') {
    return { label: t('learning.questionStateDue'), color: 'red' };
  }
  return { label: t('learning.questionStateScheduled'), color: 'green' };
}

/** 按原文位置区间截取内容；无区间时返回全文 */
export function sliceSourceContent(
  content: string,
  start: number | null,
  end: number | null
): string {
  if (start === null && end === null) return content;
  const chars = Array.from(content);
  const from = Math.max(0, start ?? 0);
  const to = Math.min(chars.length, end ?? chars.length);
  if (from >= to) return content;
  return chars.slice(from, to).join('');
}

export interface StoredReviewFilters {
  courses: string[];
  tags: string[];
}

export function loadStoredReviewFilters(): StoredReviewFilters {
  try {
    const raw = localStorage.getItem(REVIEW_FILTERS_STORAGE_KEY);
    if (raw !== null) {
      const parsed: unknown = JSON.parse(raw);
      if (parsed && typeof parsed === 'object') {
        const { courses, tags } = parsed as { courses?: unknown; tags?: unknown };
        return {
          courses: Array.isArray(courses) ? courses.filter((c): c is string => typeof c === 'string') : [],
          tags: Array.isArray(tags) ? tags.filter((t): t is string => typeof t === 'string') : [],
        };
      }
    }
  } catch {
    // Corrupted storage falls back to empty filters.
  }
  return { courses: [], tags: [] };
}

export function loadVisibleQuestionColumns(): string[] {
  try {
    const raw = localStorage.getItem(QUESTION_COLUMNS_STORAGE_KEY);
    if (raw !== null) {
      const parsed: unknown = JSON.parse(raw);
      if (Array.isArray(parsed)) {
        return QUESTION_SELECTABLE_COLUMNS.filter((key) => parsed.includes(key));
      }
    }
  } catch {
    // Corrupted storage falls back to the default column set.
  }
  return [...QUESTION_SELECTABLE_COLUMNS];
}

/** 统一的异步错误文案：Error 取 message，其余回退到通用失败提示 */
export function errorMessage(t: Translate, error: unknown): string {
  return error instanceof Error ? error.message : t('learning.actionFailed');
}

/** 题目表单校验；返回 i18n 键，校验通过返回 null */
export function validateQuestionForm(
  prompt: string,
  options: string[],
  answer: unknown,
  isSingleChoice: boolean
): string | null {
  if (prompt.trim().length === 0) {
    return 'learning.questionPromptRequired';
  }
  const cleanedOptions = options.map((option) => option.trim()).filter((option) => option !== '');
  if (isSingleChoice) {
    if (cleanedOptions.length < 2) {
      return 'learning.questionOptionsRequired';
    }
    if (typeof answer !== 'string' || !cleanedOptions.includes(answer)) {
      return 'learning.questionAnswerInvalid';
    }
  } else if (typeof answer !== 'boolean') {
    return 'learning.questionAnswerInvalid';
  }
  return null;
}
