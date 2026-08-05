import type { ConversationId } from '@/common/types/ids';
import { ipcBridge } from '@/common';
import type { GoalContractDto, GoalStatusResponse } from '@/common/adapter/ipcBridge';
import type { GoalSlashInvocation } from '@/common/chat/slash/goalCommand';
import { emitter, useAddEventListener } from '@/renderer/utils/emitter';
import { Message } from '@arco-design/web-react';
import React, { useCallback, useEffect, useState } from 'react';
import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';

const OBJECTIVE_TOAST_MAX_CHARS = 60;

// draft 是 LLM 调用，会慢；用固定 id 的 loading toast，完成/失败时原地替换。
const DRAFT_LOADING_TOAST_ID = 'goal-draft-loading';

function summarizeObjective(objective: string): string {
  const normalized = objective.replace(/\s+/g, ' ').trim();
  if (normalized.length <= OBJECTIVE_TOAST_MAX_CHARS) {
    return normalized;
  }
  return `${normalized.slice(0, OBJECTIVE_TOAST_MAX_CHARS)}…`;
}

/** [label, value] pairs for the contract fields that carry a value. */
export function goalContractEntries(contract: GoalContractDto, t: TFunction): Array<[string, string]> {
  const fields: Array<[string, string]> = [
    [t('conversation.goal.contract.outcome'), contract.outcome],
    [t('conversation.goal.contract.verification'), contract.verification],
    [t('conversation.goal.contract.constraints'), contract.constraints],
    [t('conversation.goal.contract.boundaries'), contract.boundaries],
    [t('conversation.goal.contract.stopWhen'), contract.stop_when],
  ];
  return fields.filter(([, value]) => value.trim().length > 0);
}

/** Multi-line toast body: title + one line per non-empty contract field. */
function contractToastContent(title: string, contract: GoalContractDto, t: TFunction): React.ReactNode {
  return React.createElement(
    'div',
    { style: { textAlign: 'left' } },
    React.createElement('div', { key: 'title' }, title),
    ...goalContractEntries(contract, t).map(([label, value], i) =>
      React.createElement('div', { key: i }, `${label}: ${summarizeObjective(value)}`)
    )
  );
}

/** One-line description of the active wait barrier, if any. */
function waitBarrierLine(status: GoalStatusResponse, t: TFunction): string | null {
  if (status.status !== 'waiting') return null;
  if (status.waiting_on_session) {
    return t('conversation.goal.notice.waitingOnSession', { session: status.waiting_on_session });
  }
  if (status.waiting_on_pid != null) {
    return t('conversation.goal.notice.waitingOnPid', { pid: status.waiting_on_pid });
  }
  if (status.waiting_until != null) {
    return t('conversation.goal.notice.waiting', { objective: '' }).trim();
  }
  return null;
}

function backendErrorMessage(error: unknown): string | null {
  if (error instanceof Error && error.message) {
    return error.message;
  }
  if (typeof error === 'string' && error) {
    return error;
  }
  return null;
}

/**
 * Executes a parsed `/goal` or `/subgoal` invocation against the goal API,
 * surfaces a light Arco toast as user feedback and notifies GoalStatusNotice
 * via the `goal.status.refresh` event (carrying the fresh snapshot so the
 * notice updates without an extra GET).
 *
 * `enabled` should be gated by the conversation type — the backend goal
 * endpoint only supports the nomi runtime.
 */
export function useGoalCommand(conversation_id?: ConversationId, enabled = true) {
  const { t } = useTranslation();

  const run = useCallback(
    async (
      invocation: GoalSlashInvocation,
      // `setToast` 区分 set 成功后的两种后续：'started'（首轮立即发送）与
      // 'deferred'（会话忙碌，当前回合结束后由 judge 接管续作）。
      options?: { setToast?: 'started' | 'deferred' }
    ): Promise<boolean> => {
      if (!conversation_id) {
        return false;
      }
      // `start` is a composer-only action. SendBox arms goal mode and it must
      // never reach the goal API.
      if (invocation.action === 'start') {
        return false;
      }
      // Malformed `/subgoal remove <n>` / `/goal wait <pid>` — hint the user
      // without a request.
      if (invocation.action === 'invalid_subgoal_index') {
        Message.error(t('conversation.goal.toast.subgoalInvalidIndex'));
        return false;
      }
      if (invocation.action === 'invalid_wait_pid') {
        Message.error(t('conversation.goal.toast.waitInvalidPid'));
        return false;
      }
      // Bare `/subgoal add` — an empty subgoal text, hinted locally.
      if (invocation.action === 'invalid_subgoal_text') {
        Message.error(t('conversation.goal.toast.subgoalEmptyText'));
        return false;
      }
      if (invocation.action === 'draft') {
        Message.loading({ id: DRAFT_LOADING_TOAST_ID, content: t('conversation.goal.toast.drafting'), duration: 0 });
      }
      try {
        const status = await ipcBridge.conversation.goalAction.invoke({
          conversation_id,
          action: invocation.action,
          objective:
            invocation.action === 'set' || invocation.action === 'draft' ? invocation.objective : undefined,
          subgoal: invocation.action === 'add_subgoal' ? invocation.subgoal : undefined,
          index_1based: invocation.action === 'remove_subgoal' ? invocation.index_1based : undefined,
          pid: invocation.action === 'wait' ? invocation.pid : undefined,
          contract: invocation.action === 'set_contract' ? invocation.contract : undefined,
        });
        emitter.emit('goal.status.refresh', { conversation_id, status });
        switch (invocation.action) {
          case 'set':
            Message.success(
              t(
                options?.setToast === 'deferred'
                  ? 'conversation.goal.toast.setDeferred'
                  : 'conversation.goal.toast.setStarted',
                { objective: summarizeObjective(invocation.objective) }
              )
            );
            break;
          case 'pause':
            Message.success(t('conversation.goal.toast.paused'));
            break;
          case 'resume':
            Message.success(t('conversation.goal.toast.resumed'));
            break;
          case 'clear':
            Message.success(t('conversation.goal.toast.cleared'));
            break;
          case 'status':
            if (status?.active && status.objective) {
              // 目标一行 + 裁决/回合预算一行 + 屏障行（若在等待）。
              const lines: React.ReactNode[] = [
                React.createElement(
                  'div',
                  { key: 'objective' },
                  t('conversation.goal.toast.status', { objective: summarizeObjective(status.objective) })
                ),
                React.createElement(
                  'div',
                  { key: 'detail' },
                  t('conversation.goal.toast.statusDetailed', {
                    verdict: status.last_verdict ?? '—',
                    used: status.turns_used ?? 0,
                    max: status.max_turns ?? '—',
                  })
                ),
              ];
              const barrier = waitBarrierLine(status, t);
              if (barrier) {
                lines.push(React.createElement('div', { key: 'barrier' }, barrier));
              }
              Message.info({
                content: React.createElement('div', { style: { textAlign: 'left' } }, ...lines),
              });
            } else {
              Message.info(t('conversation.goal.toast.statusNone'));
            }
            break;
          case 'add_subgoal':
            Message.success(
              t('conversation.goal.toast.subgoalAdded', { subgoal: summarizeObjective(invocation.subgoal) })
            );
            break;
          case 'remove_subgoal':
            Message.success(t('conversation.goal.toast.subgoalRemoved', { index: invocation.index_1based }));
            break;
          case 'clear_subgoals':
            Message.success(t('conversation.goal.toast.subgoalsCleared'));
            break;
          case 'list_subgoals': {
            const subgoals = status?.subgoals ?? [];
            if (!status?.active) {
              Message.info(t('conversation.goal.toast.statusNone'));
            } else if (subgoals.length === 0) {
              Message.info(t('conversation.goal.toast.subgoalListEmpty'));
            } else {
              // Numbered, multi-line list (numbering matches `/subgoal remove <n>`).
              Message.info({
                content: React.createElement(
                  'div',
                  { style: { textAlign: 'left' } },
                  React.createElement(
                    'div',
                    { key: 'title' },
                    t('conversation.goal.toast.subgoalListTitle', { count: subgoals.length })
                  ),
                  ...subgoals.map((subgoal, i) =>
                    React.createElement('div', { key: i }, `${i + 1}. ${summarizeObjective(subgoal)}`)
                  )
                ),
              });
            }
            break;
          }
          case 'draft':
            if (status?.contract) {
              Message.success({
                id: DRAFT_LOADING_TOAST_ID,
                content: contractToastContent(t('conversation.goal.toast.drafted'), status.contract, t),
              });
            } else {
              Message.success({ id: DRAFT_LOADING_TOAST_ID, content: t('conversation.goal.toast.drafted') });
            }
            break;
          case 'set_contract':
            Message.success(t('conversation.goal.toast.contractSet'));
            break;
          case 'show':
            if (!status?.active) {
              Message.info(t('conversation.goal.toast.statusNone'));
            } else {
              // 目标一行 + 契约字段行（若有）+ 屏障行（若在等待）。
              const lines: React.ReactNode[] = [
                React.createElement(
                  'div',
                  { key: 'objective' },
                  t('conversation.goal.toast.status', { objective: summarizeObjective(status.objective ?? '') })
                ),
              ];
              if (status.contract) {
                const entries = goalContractEntries(status.contract, t);
                if (entries.length > 0) {
                  lines.push(
                    ...entries.map(([label, value], i) =>
                      React.createElement('div', { key: `c-${i}` }, `${label}: ${summarizeObjective(value)}`)
                    )
                  );
                }
              } else {
                lines.push(
                  React.createElement('div', { key: 'no-contract' }, t('conversation.goal.toast.noContract'))
                );
              }
              const barrier = waitBarrierLine(status, t);
              if (barrier) {
                lines.push(React.createElement('div', { key: 'barrier' }, barrier));
              }
              Message.info({
                content: React.createElement('div', { style: { textAlign: 'left' } }, ...lines),
              });
            }
            break;
          case 'wait':
            Message.success(t('conversation.goal.toast.waitSet', { pid: invocation.pid }));
            break;
          case 'unwait':
            Message.success(t('conversation.goal.toast.unwaitDone'));
            break;
        }
        return true;
      } catch (error) {
        console.error('[useGoalCommand] Goal action failed:', error);
        // 后端的 400/502 文本（如 "No goal is set…"）比通用失败文案更有用。
        const detail = backendErrorMessage(error);
        const content = detail
          ? t('conversation.goal.toast.failedWithReason', { reason: detail })
          : t('conversation.goal.toast.failed');
        if (invocation.action === 'draft') {
          Message.error({ id: DRAFT_LOADING_TOAST_ID, content });
        } else {
          Message.error(content);
        }
        return false;
      }
    },
    [conversation_id, t]
  );

  return { enabled: enabled && Boolean(conversation_id), run };
}

/**
 * Live goal snapshot for a conversation surface. Refreshes on mount and after
 * every `/goal` / `/subgoal` action (via the `goal.status.refresh` event that
 * carries the fresh snapshot, avoiding a redundant GET). `goal` is `null`
 * while loading and `{ active: false }` when the conversation has no goal.
 * Shared by GoalStatusNotice and the composer goal-mode chip.
 */
export function useGoalStatus(conversation_id: ConversationId) {
  const [goal, setGoal] = useState<GoalStatusResponse | null>(null);

  const refresh = useCallback(async () => {
    try {
      const status = await ipcBridge.conversation.getGoalStatus.invoke({ conversation_id });
      setGoal(status ?? null);
    } catch (error) {
      console.warn('[useGoalStatus] Failed to load goal status:', error);
    }
  }, [conversation_id]);

  useEffect(() => {
    setGoal(null);
    void refresh();
  }, [refresh]);

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

  return { goal, refresh };
}
