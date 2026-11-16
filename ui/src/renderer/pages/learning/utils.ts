import { useTranslation } from 'react-i18next';
import { isBackendHttpError, isBackendRequestError } from '@/common/adapter/httpBridge';
import {
  ARCHIVE_STABLE_DAYS,
  ARCHIVE_STABLE_REVIEW_COUNT,
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
    skipped: t('learning.learningGraphStatusSkipped'),
  };
  return labels[status];
}

export function formatReviewTime(value: number | null): string {
  return value === null ? '—' : new Date(value).toLocaleString();
}

/** 库 id → 库名（t4 来源披露共用；解析不到回退「来源库已删除」的人面词典
 * 文案——裸 UUID 不再上屏（o4）；不猜名也不静默）。生成视图与悬浮指示条
 * 两处共用，口径一份。 */
export function knowledgeBaseNames(
  ids: string[],
  bases: { knowledge_base_id: string; name: string }[],
  t: Translate
): string[] {
  return ids.map(
    (id) =>
      bases.find((base) => base.knowledge_base_id === id)?.name ?? t('learning.sourceLibraryDeleted')
  );
}

export function questionStateMeta(
  entry: QuestionEntry,
  t: (key: string) => string
): { label: string; color: string } {
  if (entry.state === 'archived') {
    return { label: t('learning.questionStateArchived'), color: 'gray' };
  }
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

/**
 * 归档前判断"学习算法数据是否达标"：复习次数与记忆稳定性任一低于阈值即
 * 视为未达标。达标规则集中在常量中，刷卡界面与问题管理共用。
 */
export function isReviewUnderdeveloped(review: {
  review_count: number;
  stability_days: number;
}): boolean {
  return (
    review.review_count < ARCHIVE_STABLE_REVIEW_COUNT ||
    review.stability_days < ARCHIVE_STABLE_DAYS
  );
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

/**
 * 统一的异步错误文案单源（o1）：`BackendHttpError` 按 status 映射学习域
 * 词典（409 冲突 / 404 内容缺失 / 5xx 服务不可用），传输层故障（超时/断网）
 * 映射网络句，其余一律通用失败句。后端错误原文（含契约门报文）一律不上
 * UI，只在 console.debug 留诊断。语义与 d7 的 409 拦截路径（inflightTaskToast
 * 同水位）对齐——d7 拦截先行命中时根本不会走到这里，零改动保留。
 */
export function errorMessage(t: Translate, error: unknown): string {
  // 诊断原文只进 console；UI 面永远是词典整句。
  console.debug('[learning:errorMessage] mapped to friendly copy, raw:', error);
  if (isBackendHttpError(error)) {
    if (error.status === 409) return t('learning.conflictBusy');
    if (error.status === 404) return t('learning.contentMissing');
    if (error.status >= 500) return t('learning.serviceUnavailable');
    return t('learning.actionFailed');
  }
  if (isBackendRequestError(error)) {
    // timeout / network / aborted 都是传输面故障，人面同一句。
    return t('learning.networkUnstable');
  }
  return t('learning.actionFailed');
}

/**
 * quiz 开测 409 的 K 值提取（u5 R2）：due 门 Conflict 报文恒为
 * `quiz not due: {N}/{K} settlements since last quiz`（quiz.rs 单源格式），
 * K = 开放阈值——提取后填词典 quizNotDueHint 的 {{trigger}} 槽位，N/K 读数
 * 本身由 onConflict 触发的进度重拉呈现（读数面不靠报文）。报文是后端散文
 * 镜像，格式漂移提取不到数时返回 null（调用方回落 conflictBusy 通用句）；
 * 非 409 或非 BackendHttpError 一律 null。原文不直接上屏。
 */
export function quizNotDueTrigger(error: unknown): number | null {
  if (!isBackendHttpError(error) || error.status !== 409) return null;
  const match = /quiz not due:\s*\d+\/(\d+)/.exec(error.backendMessage);
  return match ? Number(match[1]) : null;
}

/** 槽冲突（409 CONFLICT，d6/d7）：同类长任务已在后台跑。后端错误文本恒含
 * 槽 key（`course generation for this source is already running (…)`）与
 * `CONFLICT` 码；keyFragment 用于精确认领（如 `basis-declare-{courseId}`）
 * ——命中即走非阻塞提示 + 数据重拉，不弹原始 JSON 错误。 */
export function isGenerationConflict(error: unknown, keyFragment?: string): boolean {
  const message = error instanceof Error ? error.message : String(error);
  if (!message.includes('CONFLICT') && !message.includes('(409)')) return false;
  if (!message.includes('already running')) return false;
  return keyFragment ? message.includes(keyFragment) : true;
}

/** 题目表单校验；返回 i18n 键，校验通过返回 null */
export function validateQuestionForm(
  prompt: string,
  options: string[],
  answer: unknown,
  isSingleChoice: boolean,
  isFillInBlank = false
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
  } else if (isFillInBlank) {
    if (!prompt.includes('___')) {
      return 'learning.questionFillBlankHintRequired';
    }
    if (!Array.isArray(answer) || answer.length === 0 || answer.length > 3) {
      return 'learning.questionAnswerInvalid';
    }
    if (answer.some((item) => typeof item !== 'string' || item.trim().length === 0)) {
      return 'learning.questionAnswerInvalid';
    }
  } else if (typeof answer !== 'boolean') {
    return 'learning.questionAnswerInvalid';
  }
  return null;
}
