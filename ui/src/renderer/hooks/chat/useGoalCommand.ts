import type { ConversationId } from '@/common/types/ids';
import { ipcBridge } from '@/common';
import type { GoalSlashInvocation } from '@/common/chat/slash/goalCommand';
import { emitter } from '@/renderer/utils/emitter';
import { Message } from '@arco-design/web-react';
import { useCallback } from 'react';
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
 * Executes a parsed `/goal` invocation against the goal API, surfaces a light
 * Arco toast as user feedback and notifies GoalStatusNotice via the
 * `goal.status.refresh` event (carrying the fresh snapshot so the notice
 * updates without an extra GET).
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
      try {
        const status = await ipcBridge.conversation.goalAction.invoke({
          conversation_id,
          action: invocation.action,
          objective: invocation.action === 'set' ? invocation.objective : undefined,
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
