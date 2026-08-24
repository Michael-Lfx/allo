/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { conversationTarget } from '@/common/types/ids';
import { Button, Tooltip } from '@arco-design/web-react';
import { Branch } from '@icon-park/react';
import React, { useEffect, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import ChatLayout, { type ChatLayoutProps } from '../components/ChatLayout';
import { dispatchWorkspaceOpenPreviewTool } from '../components/ChatLayout/WorkspaceToolRail';
import ExecutionContentSwitcher from './ExecutionContentSwitcher';
import { useExecution } from './ExecutionContext';
import { EXECUTION_STATUS_META } from './executionStatusMeta';
import ExecutionTopPanel from './ExecutionTopPanel';
import PlanApprovalBanner from './PlanApprovalBanner';

export const AGENT_EXECUTION_WORKSPACE_TAB = 'agent-execution';

/**
 * Conversation-native shell for the one AgentExecution projection.
 *
 * Every authorized locally hosted Agent can delegate through its process-issued
 * Platform Gateway capability, so the
 * execution chrome is deliberately independent of the conversation runtime.
 * Runtime-specific controls still belong to the child composer; progress,
 * decisions and lifecycle commands are available wherever an authoritative
 * ConversationExecutionLink projects an active execution.
 *
 * Collaboration canvas lives in the shared preview tab strip (not a chat-side
 * split), matching Files / Changes / Shell.
 */
const ExecutionConversationLayout: React.FC<ChatLayoutProps> = ({
  children,
  headerExtra,
  workspaceExtraTabs,
  conversation_id,
  ...layoutProps
}) => {
  const { t } = useTranslation();
  const execution = useExecution();
  const status = execution.detail?.execution.status ?? '';
  const workspaceTarget = conversation_id != null ? conversationTarget(conversation_id) : undefined;

  const openCollaborationTab = () => {
    if (!workspaceTarget) return;
    dispatchWorkspaceOpenPreviewTool(AGENT_EXECUTION_WORKSPACE_TAB, workspaceTarget);
  };

  // When an execution appears, surface it in the preview strip (same moment the
  // old side canvas used to auto-open).
  useEffect(() => {
    if (!execution.executionId || !workspaceTarget) return;
    dispatchWorkspaceOpenPreviewTool(AGENT_EXECUTION_WORKSPACE_TAB, workspaceTarget);
  }, [execution.executionId, workspaceTarget?.id, workspaceTarget?.kind]);

  const mergedExtraTabs = useMemo(() => {
    const base = workspaceExtraTabs ?? [];
    if (!execution.executionId) {
      return base.filter((tab) => tab.key !== AGENT_EXECUTION_WORKSPACE_TAB);
    }
    if (base.some((tab) => tab.key === AGENT_EXECUTION_WORKSPACE_TAB)) return base;
    return [
      ...base,
      {
        key: AGENT_EXECUTION_WORKSPACE_TAB,
        title: t('agentExecution.panel.title', { defaultValue: '协作任务' }),
        icon: <Branch size={18} />,
        content: <ExecutionTopPanel embedded />,
      },
    ];
  }, [execution.executionId, t, workspaceExtraTabs]);

  return (
    <ChatLayout
      {...layoutProps}
      conversation_id={conversation_id}
      workspaceExtraTabs={mergedExtraTabs}
      headerExtra={
        <div className='flex items-center gap-8px'>
          {headerExtra}
          {execution.executionId && (
            <Tooltip content={t('agentExecution.panel.open', { defaultValue: '打开协作任务' })}>
              <Button
                size='mini'
                type='default'
                aria-label={t('agentExecution.panel.open', { defaultValue: '打开协作任务' })}
                icon={<Branch theme='outline' size='14' strokeWidth={3} />}
                onClick={openCollaborationTab}
              />
            </Tooltip>
          )}
        </div>
      }
      workspaceCollaboration={{
        active: false,
        available: Boolean(execution.executionId),
        statusColor: EXECUTION_STATUS_META[status as keyof typeof EXECUTION_STATUS_META]?.color,
        onClick: openCollaborationTab,
      }}
    >
      <div className='relative flex flex-row flex-1 min-h-0' data-testid='conversation-execution-layout'>
        <div className='flex-1 min-w-0 min-h-0 flex flex-col'>
          <PlanApprovalBanner />
          <ExecutionContentSwitcher>{children}</ExecutionContentSwitcher>
        </div>
      </div>
    </ChatLayout>
  );
};

export default ExecutionConversationLayout;
