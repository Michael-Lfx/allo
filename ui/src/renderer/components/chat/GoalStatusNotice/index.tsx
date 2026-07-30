import type { ConversationId } from '@/common/types/ids';
import { ipcBridge } from '@/common';
import type { GoalStatusResponse } from '@/common/adapter/ipcBridge';
import { goalContractEntries, useGoalCommand } from '@/renderer/hooks/chat/useGoalCommand';
import { useAddEventListener } from '@/renderer/utils/emitter';
import { Button, Popconfirm } from '@arco-design/web-react';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';

// 仅在 goal 自动续作期间低频轮询（turns_used 随每个 turn 结束而变化，且没有
// goal 专属的 WS 事件可订阅）；其余状态只靠进入会话时的 GET + 操作后的事件刷新。
// Poll at low frequency only while the goal auto-continues (turns_used moves
// after every turn and there is no goal-specific WS event to subscribe to);
// otherwise rely on the mount GET + post-action refresh events.
const ACTIVE_POLL_INTERVAL_MS = 15_000;

/** h:mm:ss above one hour, m:ss otherwise. */
function formatRemaining(ms: number): string {
  const totalSeconds = Math.max(0, Math.ceil(ms / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  const pad = (n: number) => String(n).padStart(2, '0');
  return hours > 0 ? `${hours}:${pad(minutes)}:${pad(seconds)}` : `${minutes}:${pad(seconds)}`;
}

/**
 * Goal status + progress rail for the conversation surface, mirroring
 * TurnStatusRail's visual language (dot + 12px secondary text). Beyond the
 * one-line status it renders a turns progress bar, pause/resume/clear
 * controls (wired through useGoalCommand), the wait barrier while
 * `status === 'waiting'` (session > pid > timed countdown, with an unwait
 * control), a collapsible completion-contract block and a collapsible
 * numbered subgoal list (numbering matches `/subgoal remove <n>`). Renders
 * nothing when the conversation has no goal snapshot.
 */
const GoalStatusNotice: React.FC<{ conversation_id: ConversationId }> = ({ conversation_id }) => {
  const { t } = useTranslation();
  const [goal, setGoal] = useState<GoalStatusResponse | null>(null);
  const [subgoalsExpanded, setSubgoalsExpanded] = useState(false);
  const [contractExpanded, setContractExpanded] = useState(false);
  const goalCommand = useGoalCommand(conversation_id);

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
    setSubgoalsExpanded(false);
    setContractExpanded(false);
    void refresh();
  }, [refresh]);

  // /goal 与 /subgoal 操作后由 useGoalCommand 发出；快照随事件携带时直接采用，
  // 免去一次 GET。
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

  // 等待屏障倒计时：仅 waiting 且带 waiting_until 时每秒走一格。到 0 后后端是
  // 懒恢复（下一回合才实际继续），因此归零显示"等待结束，将在下一回合恢复"。
  const waitingUntil = goal?.active && goal.status === 'waiting' ? goal.waiting_until : undefined;
  const [nowMs, setNowMs] = useState(() => Date.now());
  useEffect(() => {
    if (waitingUntil == null) return;
    setNowMs(Date.now());
    const timer = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [waitingUntil]);

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

  if (!presentation || !goal) {
    return null;
  }

  const hasTurns = typeof goal.turns_used === 'number' && typeof goal.max_turns === 'number';
  const showTurns = (goal.status === 'active' || goal.status === 'waiting') && hasTurns;
  const showProgress =
    hasTurns && (goal.status === 'active' || goal.status === 'waiting' || goal.status === 'paused');
  const progressPercent =
    showProgress && goal.max_turns! > 0 ? Math.min(100, (goal.turns_used! / goal.max_turns!) * 100) : 0;

  const canPause = goal.status === 'active' || goal.status === 'waiting';
  const canResume = goal.status === 'paused';
  const canClear = goal.status !== 'cleared';

  const remainingMs = waitingUntil != null ? waitingUntil - nowMs : undefined;
  const subgoals = goal.subgoals ?? [];
  // 屏障可能并存；主行按 session > pid > time 优先级展示，其余并列在同一行。
  const isWaiting = goal.status === 'waiting';
  const barrierLines: string[] = [];
  if (isWaiting) {
    if (goal.waiting_on_session) {
      barrierLines.push(t('conversation.goal.notice.waitingOnSession', { session: goal.waiting_on_session }));
    }
    if (goal.waiting_on_pid != null) {
      barrierLines.push(t('conversation.goal.notice.waitingOnPid', { pid: goal.waiting_on_pid }));
    }
    if (remainingMs != null) {
      barrierLines.push(
        remainingMs > 0
          ? t('conversation.goal.notice.waitingCountdown', { time: formatRemaining(remainingMs) })
          : t('conversation.goal.notice.waitElapsed')
      );
    }
  }
  const contractEntries = goal.contract ? goalContractEntries(goal.contract, t) : [];

  return (
    <div
      className='goal-status-notice mx-auto mb-4px max-w-780px w-full px-8px text-12px text-t-secondary flex flex-col gap-4px'
      role='status'
      aria-live='polite'
      data-testid='goal-status-notice'
      data-goal-status={goal.status}
    >
      <div className='flex items-center gap-8px min-h-20px'>
        <span className={`inline-block w-6px h-6px rd-full shrink-0 ${presentation.dotClass}`} aria-hidden='true' />
        <span className='truncate'>{presentation.label}</span>
        {showTurns && (
          <span className='shrink-0 text-t-tertiary'>
            {t('conversation.goal.notice.turns', { used: goal.turns_used, max: goal.max_turns })}
          </span>
        )}
        <span className='ml-auto shrink-0 flex items-center gap-2px'>
          {canPause && (
            <Button size='mini' type='text' onClick={() => void goalCommand.run({ action: 'pause' })}>
              {t('conversation.goal.notice.pause')}
            </Button>
          )}
          {canResume && (
            <Button size='mini' type='text' onClick={() => void goalCommand.run({ action: 'resume' })}>
              {t('conversation.goal.notice.resume')}
            </Button>
          )}
          {canClear && (
            <Popconfirm
              title={t('conversation.goal.notice.clearConfirm')}
              onOk={() => void goalCommand.run({ action: 'clear' })}
            >
              <Button size='mini' type='text' status='danger'>
                {t('conversation.goal.notice.clearAction')}
              </Button>
            </Popconfirm>
          )}
        </span>
      </div>
      {showProgress && (
        <div
          className='h-4px w-full rd-full bg-6 overflow-hidden'
          role='progressbar'
          aria-valuemin={0}
          aria-valuemax={goal.max_turns}
          aria-valuenow={goal.turns_used}
          data-testid='goal-progress-bar'
        >
          <div
            className='h-full rd-full bg-primary transition-width transition-duration-300'
            style={{ width: `${progressPercent}%` }}
          />
        </div>
      )}
      {isWaiting && (
        <div className='flex items-center gap-8px text-t-tertiary' data-testid='goal-wait-barrier'>
          {barrierLines.length > 0 && <span className='truncate'>{barrierLines.join(' · ')}</span>}
          {goal.waiting_reason && (
            <span className='truncate'>{t('conversation.goal.notice.reason', { reason: goal.waiting_reason })}</span>
          )}
          <Button
            className='shrink-0'
            size='mini'
            type='text'
            onClick={() => void goalCommand.run({ action: 'unwait' })}
          >
            {t('conversation.goal.notice.unwait')}
          </Button>
        </div>
      )}
      {contractEntries.length > 0 && (
        <div data-testid='goal-contract'>
          <button
            type='button'
            className='bg-transparent border-none p-0 cursor-pointer text-12px text-t-tertiary hover:text-t-secondary'
            aria-expanded={contractExpanded}
            onClick={() => setContractExpanded((v) => !v)}
          >
            {`${contractExpanded ? '▾' : '▸'} ${t('conversation.goal.notice.contractToggle')}`}
          </button>
          {contractExpanded && (
            <div className='mt-2px pl-16px flex flex-col gap-2px text-t-tertiary'>
              {contractEntries.map(([label, value], index) => (
                <div key={index} className='truncate'>
                  {`${label}: ${value}`}
                </div>
              ))}
            </div>
          )}
        </div>
      )}
      {subgoals.length > 0 && (
        <div data-testid='goal-subgoals'>
          <button
            type='button'
            className='bg-transparent border-none p-0 cursor-pointer text-12px text-t-tertiary hover:text-t-secondary'
            aria-expanded={subgoalsExpanded}
            onClick={() => setSubgoalsExpanded((v) => !v)}
          >
            {`${subgoalsExpanded ? '▾' : '▸'} ${t('conversation.goal.notice.subgoalsToggle', { count: subgoals.length })}`}
          </button>
          {subgoalsExpanded && (
            <ol className='m-0 mt-2px pl-16px flex flex-col gap-2px text-t-tertiary'>
              {subgoals.map((subgoal, index) => (
                <li key={index} className='truncate' value={index + 1}>
                  {subgoal}
                </li>
              ))}
            </ol>
          )}
        </div>
      )}
    </div>
  );
};

export default GoalStatusNotice;
