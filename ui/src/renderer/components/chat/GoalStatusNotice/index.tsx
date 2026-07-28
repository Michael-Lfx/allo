import type { ConversationId } from '@/common/types/ids';
import { ipcBridge } from '@/common';
import type { GoalStatusResponse } from '@/common/adapter/ipcBridge';
import { useAddEventListener } from '@/renderer/utils/emitter';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';

// 仅在 goal 自动续作期间低频轮询（turns_used 随每个 turn 结束而变化，且没有
// goal 专属的 WS 事件可订阅）；其余状态只靠进入会话时的 GET + 操作后的事件刷新。
// Poll at low frequency only while the goal auto-continues (turns_used moves
// after every turn and there is no goal-specific WS event to subscribe to);
// otherwise rely on the mount GET + post-action refresh events.
const ACTIVE_POLL_INTERVAL_MS = 15_000;

/**
 * One-line goal status rail for the conversation surface, mirroring
 * TurnStatusRail's visual language (dot + 12px secondary text).
 * Renders nothing when the conversation has no goal snapshot.
 */
const GoalStatusNotice: React.FC<{ conversation_id: ConversationId }> = ({ conversation_id }) => {
  const { t } = useTranslation();
  const [goal, setGoal] = useState<GoalStatusResponse | null>(null);

  const refresh = useCallback(async () => {
    try {
      const status = await ipcBridge.conversation.getGoalStatus.invoke({ conversation_id });
      setGoal(status ?? null);
    } catch (error) {
      console.warn('[GoalStatusNotice] Failed to load goal status:', error);
    }
  }, [conversation_id]);

  useEffect(() => {
    setGoal(null);
    void refresh();
  }, [refresh]);

  // /goal 操作后由 useGoalCommand 发出；快照随事件携带时直接采用，免去一次 GET。
  useAddEventListener(
    'goal.status.refresh',
    (payload) => {
      if (payload.conversation_id !== conversation_id) return;
      if (payload.status) {
        setGoal(payload.status);
      } else {
        void refresh();
      }
    },
    [conversation_id, refresh]
  );

  const isRunning = goal?.active === true && (goal.status === 'active' || goal.status === 'waiting');
  useEffect(() => {
    if (!isRunning) return;
    const timer = window.setInterval(() => {
      void refresh();
    }, ACTIVE_POLL_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, [isRunning, refresh]);

  const presentation = useMemo(() => {
    if (!goal?.active || !goal.status) {
      return null;
    }
    const objective = goal.objective ?? '';
    switch (goal.status) {
      case 'active':
        return {
          dotClass: 'bg-primary animate-pulse',
          label: t('conversation.goal.notice.active', { objective }),
        };
      case 'waiting':
        return {
          dotClass: 'bg-primary animate-pulse',
          label: t('conversation.goal.notice.waiting', { objective }),
        };
      case 'complete':
        return {
          dotClass: 'bg-success',
          label: t('conversation.goal.notice.complete', { objective }),
        };
      case 'paused':
        return {
          dotClass: 'bg-warning',
          label:
            t('conversation.goal.notice.paused', { objective }) +
            (goal.paused_reason ? t('conversation.goal.notice.reason', { reason: goal.paused_reason }) : ''),
        };
      case 'blocked':
        return {
          dotClass: 'bg-danger',
          label:
            t('conversation.goal.notice.blocked', { objective }) +
            (goal.last_reason ? t('conversation.goal.notice.reason', { reason: goal.last_reason }) : ''),
        };
      case 'cleared':
        return {
          dotClass: 'bg-6',
          label: t('conversation.goal.notice.cleared'),
        };
      default:
        return null;
    }
  }, [goal, t]);

  if (!presentation) {
    return null;
  }

  const showTurns =
    (goal?.status === 'active' || goal?.status === 'waiting') &&
    typeof goal?.turns_used === 'number' &&
    typeof goal?.max_turns === 'number';

  return (
    <div
      className='goal-status-notice mx-auto mb-4px max-w-780px w-full px-8px text-12px text-t-secondary flex items-center gap-8px min-h-20px'
      role='status'
      aria-live='polite'
      data-testid='goal-status-notice'
      data-goal-status={goal?.status}
    >
      <span className={`inline-block w-6px h-6px rd-full shrink-0 ${presentation.dotClass}`} aria-hidden='true' />
      <span className='truncate'>{presentation.label}</span>
      {showTurns && (
        <span className='shrink-0 text-t-tertiary'>
          {t('conversation.goal.notice.turns', { used: goal?.turns_used, max: goal?.max_turns })}
        </span>
      )}
    </div>
  );
};

export default GoalStatusNotice;
