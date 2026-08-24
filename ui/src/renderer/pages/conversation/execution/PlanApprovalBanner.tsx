/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ipcBridge } from '@/common';
import { useArcoMessage } from '@/renderer/utils/ui/useArcoMessage';
import ApprovalCard from '@renderer/components/beautifulUi/approvalCard/ApprovalCard';
import { useExecutionSafe } from './ExecutionContext';
import { refreshOnVersionConflict } from './refreshOnVersionConflict';

// Toasts stay click-through so they never block the banner action.
const TOAST_OK_MS = 1500;
const TOAST_ERR_MS = 2500;

/** In-conversation approval affordance for executions waiting at their plan gate. */
const PlanApprovalBanner: React.FC = () => {
  const { t } = useTranslation();
  const execution = useExecutionSafe();
  const [message, msgCtx] = useArcoMessage();
  const [approving, setApproving] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>('approve');

  const executionId = execution?.executionId ?? null;
  const parked = execution?.detail?.execution.status === 'awaiting_approval';

  const doApprove = async () => {
    if (approving || !executionId) return;
    setApproving(true);
    try {
      await ipcBridge.agentExecution.approve.invoke({
        execution_id: executionId,
        updates: {
          expected_version: execution?.detail?.execution.version ?? 0,
        },
      });
      message.success({
        content: t('agentExecution.approval.ok', {
          defaultValue: '已批准，开始协作',
        }),
        duration: TOAST_OK_MS,
        passthrough: true,
      });
      await execution?.refetch();
    } catch (e) {
      await refreshOnVersionConflict(e, execution?.refetch ?? (async () => {}));
      message.error({
        content: t('agentExecution.approval.error', {
          defaultValue: '批准失败：{{error}}',
          error: String(e),
        }),
        duration: TOAST_ERR_MS,
        passthrough: true,
      });
    } finally {
      setApproving(false);
    }
  };

  // Only surface while the linked execution is awaiting approval.
  if (!execution || !executionId || !parked) return null;

  return (
    <div className='flex-shrink-0'>
      {msgCtx}
      <ApprovalCard
        kind='plan'
        title={t('agentExecution.approval.text', {
          defaultValue: '协作计划已就绪，可继续调整；准备好后批准执行。',
        })}
        description={t('agentExecution.approval.eyebrow', { defaultValue: '待批准' })}
        options={[{ id: 'approve', label: t('agentExecution.approval.button', { defaultValue: '批准执行' }) }]}
        selectedId={selectedId}
        onSelect={setSelectedId}
        onConfirm={() => {
          void doApprove();
        }}
        confirmLabel={t('agentExecution.approval.button', {
          defaultValue: '批准执行',
        })}
        disabled={approving}
      />
    </div>
  );
};

export default PlanApprovalBanner;
