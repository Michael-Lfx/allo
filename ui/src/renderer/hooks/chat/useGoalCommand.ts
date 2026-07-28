import type { ConversationId } from '@/common/types/ids';
import { ipcBridge } from '@/common';
import type { GoalSlashInvocation } from '@/common/chat/slash/goalCommand';
import { emitter } from '@/renderer/utils/emitter';
import { Message } from '@arco-design/web-react';
import React, { useCallback } from 'react';
import { useTranslation } from 'react-i18next';

const OBJECTIVE_TOAST_MAX_CHARS = 60;

function summarizeObjective(objective: string): string {
  const normalized = objective.replace(/\s+/g, ' ').trim();
  if (normalized.length <= OBJECTIVE_TOAST_MAX_CHARS) {
    return normalized;
  }
  return `${normalized.slice(0, OBJECTIVE_TOAST_MAX_CHARS)}…`;
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
    async (invocation: GoalSlashInvocation) => {
      if (!conversation_id) {
        return;
      }
      // Malformed `/subgoal remove <n>` — hint the user without a request.
      if (invocation.action === 'invalid_subgoal_index') {
        Message.error(t('conversation.goal.toast.subgoalInvalidIndex'));
        return;
      }
      try {
        const status = await ipcBridge.conversation.goalAction.invoke({
          conversation_id,
          action: invocation.action,
          objective: invocation.action === 'set' ? invocation.objective : undefined,
          subgoal: invocation.action === 'add_subgoal' ? invocation.subgoal : undefined,
          index_1based: invocation.action === 'remove_subgoal' ? invocation.index_1based : undefined,
        });
        emitter.emit('goal.status.refresh', { conversation_id, status });
        switch (invocation.action) {
          case 'set':
            Message.success(
              t('conversation.goal.toast.set', { objective: summarizeObjective(invocation.objective) })
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
              Message.info(
                t('conversation.goal.toast.status', { objective: summarizeObjective(status.objective) })
              );
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
        }
      } catch (error) {
        console.error('[useGoalCommand] Goal action failed:', error);
        Message.error(t('conversation.goal.toast.failed'));
      }
    },
    [conversation_id, t]
  );

  return { enabled: enabled && Boolean(conversation_id), run };
}
