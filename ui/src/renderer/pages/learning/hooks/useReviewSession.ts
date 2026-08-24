import { useCallback, useState } from 'react';
import { Message } from '@arco-design/web-react';
import { learningApi } from '../api';
import { ORPHAN_COURSE_FILTER } from '../constants';
import type { DueReview, ReviewAnswerResult, ReviewRating } from '../types';
import { errorMessage, type Translate } from '../utils';

export interface UseReviewSessionOptions {
  load: () => Promise<void>;
  t: Translate;
  reviewCourseFilter: string[];
  reviewTagFilter: string[];
  reviewSessionLimit: number | undefined;
  setBusyId: (id: string | null) => void;
  setReviews: (reviews: DueReview[]) => void;
}

/** 复习域：到期队列、会话弹窗与答题/遗忘/评分/跳过动作 */
export function useReviewSession({
  load,
  t,
  reviewCourseFilter,
  reviewTagFilter,
  reviewSessionLimit,
  setBusyId,
  setReviews,
}: UseReviewSessionOptions) {
  const [sessionOpen, setSessionOpen] = useState(false);
  const [sessionQueue, setSessionQueue] = useState<DueReview[]>([]);

  const answerReview = useCallback(
    async (review: DueReview, response: unknown): Promise<ReviewAnswerResult | undefined> => {
      setBusyId(review.id);
      try {
        return await learningApi.answerReview(review.source, review.id, response, false);
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
        return undefined;
      } finally {
        setBusyId(null);
      }
    },
    [t, setBusyId]
  );

  const forgetReview = useCallback(
    async (review: DueReview): Promise<ReviewAnswerResult | undefined> => {
      setBusyId(review.id);
      try {
        return await learningApi.answerReview(review.source, review.id, null, true);
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
        return undefined;
      } finally {
        setBusyId(null);
      }
    },
    [t, setBusyId]
  );

  const rateReview = useCallback(
    async (review: DueReview, rating: ReviewRating): Promise<boolean> => {
      setBusyId(review.id);
      try {
        await learningApi.rateReview(review.source, review.id, rating);
        Message.success(t('learning.reviewRecorded'));
        await load();
        return true;
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
        return false;
      } finally {
        setBusyId(null);
      }
    },
    [load, t, setBusyId]
  );

  const skipReview = useCallback(
    async (review: DueReview): Promise<boolean> => {
      setBusyId(review.id);
      try {
        await learningApi.skipReview(review.source, review.id);
        Message.success(t('learning.reviewSkipped'));
        await load();
        return true;
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
        return false;
      } finally {
        setBusyId(null);
      }
    },
    [load, t, setBusyId]
  );

  /** 归档当前卡片：暂停出现但保留数据，可随时恢复 */
  const archiveReview = useCallback(
    async (review: DueReview): Promise<boolean> => {
      setBusyId(review.id);
      try {
        if (review.source === 'custom') {
          await learningApi.archiveCustomQuestion(review.id);
        } else {
          await learningApi.archiveReview(review.id);
        }
        Message.success(t('learning.reviewArchived'));
        await load();
        return true;
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
        return false;
      } finally {
        setBusyId(null);
      }
    },
    [load, t, setBusyId]
  );

  /** 删除当前卡片：课程题移出队列，自定义题彻底删除 */
  const removeReview = useCallback(
    async (review: DueReview): Promise<boolean> => {
      setBusyId(review.id);
      try {
        if (review.source === 'custom') {
          await learningApi.deleteCustomQuestion(review.id);
          Message.success(t('learning.questionDeleted'));
        } else {
          await learningApi.deleteReviewItem(review.id);
          Message.success(t('learning.reviewRemovedFromQueue'));
        }
        await load();
        return true;
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
        return false;
      } finally {
        setBusyId(null);
      }
    },
    [load, t, setBusyId]
  );

  /** 标记待编辑：记录意图与选填描述，不推进队列，不打断复习心流 */
  const markEditPending = useCallback(
    async (review: DueReview, note: string): Promise<void> => {
      setBusyId(review.id);
      try {
        if (review.source === 'custom') {
          await learningApi.markCustomEditPending(review.id, note);
        } else {
          await learningApi.markReviewEditPending(review.id, note);
        }
        // 本地同步会话队列中的卡片状态，无需重拉队列
        setSessionQueue((prev) =>
          prev.map((item) =>
            item.id === review.id
              ? { ...item, edit_pending: true, edit_note: note.trim() || null }
              : item
          )
        );
        await load();
        Message.success(t('learning.reviewMarkEditMarked'));
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
      } finally {
        setBusyId(null);
      }
    },
    [load, t, setBusyId, setSessionQueue]
  );

  const startReviewSession = useCallback(async () => {
    setBusyId('review-session');
    try {
      // 每次开刷前重新拉取到期队列，避免使用会话期间过期的快照
      const isOrphan = reviewCourseFilter.includes(ORPHAN_COURSE_FILTER);
      const courseIds = reviewCourseFilter.filter((value) => value !== ORPHAN_COURSE_FILTER);
      const fresh = await learningApi.listDueReviews(reviewSessionLimit, courseIds, {
        dueOnly: true,
        orphan: isOrphan,
        tags: reviewTagFilter,
      });
      setReviews(fresh);
      if (fresh.length === 0) {
        Message.info(t('learning.noReviews'));
        return;
      }
      setSessionQueue(fresh);
      setSessionOpen(true);
    } catch (sessionError) {
      Message.error(errorMessage(t, sessionError));
    } finally {
      setBusyId(null);
    }
  }, [reviewCourseFilter, reviewTagFilter, reviewSessionLimit, t, setBusyId]);

  const startCourseReviewSession = useCallback(
    async (courseId: string) => {
      setBusyId('review-session');
      try {
        // 课程复习专注本课程全部已入队卡片，不限于到期项，因此使用更大的队列上限
        const fresh = await learningApi.listDueReviews(100, courseId);
        if (fresh.length === 0) {
          Message.info(t('learning.noCourseReviews'));
          return;
        }
        setSessionQueue(fresh);
        setSessionOpen(true);
      } catch (sessionError) {
        Message.error(errorMessage(t, sessionError));
      } finally {
        setBusyId(null);
      }
    },
    [t, setBusyId]
  );

  return {
    sessionOpen,
    setSessionOpen,
    sessionQueue,
    setSessionQueue,
    answerReview,
    forgetReview,
    rateReview,
    skipReview,
    archiveReview,
    removeReview,
    markEditPending,
    startReviewSession,
    startCourseReviewSession,
  };
}
