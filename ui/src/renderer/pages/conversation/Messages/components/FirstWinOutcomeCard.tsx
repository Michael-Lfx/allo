/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { conversationTarget, type ConversationId } from '@/common/types/ids';
import { Button } from '@arco-design/web-react';
import { CheckOne, DocDetail, FileCodeOne } from '@icon-park/react';
import React, { useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { confirmFirstValue } from '@/renderer/utils/analytics/productFunnel';
import { markFirstWinCompleted } from '@/renderer/utils/onboarding/firstWinMode';
import { emitter } from '@/renderer/utils/emitter';
import { dispatchWorkspaceToggleEvent } from '@/renderer/utils/workspace/workspaceEvents';
import type { FirstWinOutcomeSnapshot } from './firstWinOutcomeModel';

type FirstWinOutcomeCardProps = {
  snapshot: FirstWinOutcomeSnapshot;
  conversationId?: ConversationId;
  onDismiss: () => void;
};

const FirstWinOutcomeCard: React.FC<FirstWinOutcomeCardProps> = ({
  snapshot,
  conversationId,
  onDismiss,
}) => {
  const { t } = useTranslation();
  const navigate = useNavigate();

  const confirm = useCallback((source: string) => {
    confirmFirstValue({ source });
    markFirstWinCompleted();
  }, []);

  const handleContinue = useCallback(() => {
    confirm('outcome_follow_up');
    emitter.emit(
      'sendbox.fill',
      t('conversation.firstWinOutcome.followUpPrompt', {
        defaultValue: '在刚才的结果上继续：',
      })
    );
    onDismiss();
  }, [confirm, onDismiss, t]);

  const handleReviewFiles = useCallback(() => {
    confirm('outcome_review_files');
    if (conversationId) {
      dispatchWorkspaceToggleEvent(conversationTarget(conversationId));
    }
    onDismiss();
  }, [confirm, conversationId, onDismiss]);

  const handleSaveFlow = useCallback(() => {
    confirm('outcome_save_flow');
    navigate('/presets');
    onDismiss();
  }, [confirm, navigate, onDismiss]);

  const handleConfirmValue = useCallback(() => {
    confirm('outcome_confirm');
    onDismiss();
  }, [confirm, onDismiss]);

  const statusLabel =
    snapshot.status === 'with_changes'
      ? t('conversation.firstWinOutcome.statusWithChanges', {
          defaultValue: '已验证 · 有可检查的文件变更',
        })
      : t('conversation.firstWinOutcome.statusAnswer', {
          defaultValue: '已交付 · 可检查的结果摘要',
        });

  return (
    <section
      className='mt-12px mb-8px p-14px rd-10px b-1 b-solid bg-fill-0'
      style={{ borderColor: 'color-mix(in srgb, var(--color-success-6) 35%, var(--color-border-2))' }}
      data-testid='first-win-outcome-card'
      aria-label={t('conversation.firstWinOutcome.aria', { defaultValue: '首个可检查成果' })}
    >
      <div className='flex items-start gap-8px mb-10px'>
        <CheckOne theme='filled' size={18} fill='var(--color-success-6)' className='mt-1px shrink-0' />
        <div className='min-w-0 flex-1'>
          <div className='text-14px font-500 text-t-primary'>
            {t('conversation.firstWinOutcome.title', { defaultValue: '首个可检查成果' })}
          </div>
          <div className='text-12px text-t-secondary mt-2px' data-testid='first-win-outcome-status'>
            {statusLabel}
          </div>
        </div>
      </div>

      {snapshot.summary ? (
        <p className='m-0 mb-10px text-13px text-t-primary leading-relaxed' data-testid='first-win-outcome-summary'>
          {snapshot.summary}
        </p>
      ) : null}

      {snapshot.files.length > 0 ? (
        <ul className='m-0 mb-12px p-0 list-none flex flex-col gap-4px' data-testid='first-win-outcome-files'>
          {snapshot.files.map((file) => (
            <li
              key={file.path}
              className='flex items-center gap-6px text-12px text-t-secondary min-w-0'
            >
              <FileCodeOne theme='outline' size={14} className='shrink-0' />
              <span className='truncate'>{file.name}</span>
              <span className='shrink-0 text-success-6'>+{file.insertions}</span>
              <span className='shrink-0 text-danger-6'>-{file.deletions}</span>
            </li>
          ))}
        </ul>
      ) : (
        <div className='mb-12px flex items-center gap-6px text-12px text-t-secondary'>
          <DocDetail theme='outline' size={14} />
          <span>
            {t('conversation.firstWinOutcome.noFilesHint', {
              defaultValue: '暂无文件变更 · 可复制摘要或继续追问确认价值',
            })}
          </span>
        </div>
      )}

      <div className='flex flex-wrap gap-8px'>
        <Button type='primary' size='small' data-testid='first-win-outcome-continue' onClick={handleContinue}>
          {t('conversation.firstWinOutcome.continue', { defaultValue: '继续追问' })}
        </Button>
        {snapshot.files.length > 0 ? (
          <Button size='small' data-testid='first-win-outcome-review' onClick={handleReviewFiles}>
            {t('conversation.firstWinOutcome.reviewFiles', { defaultValue: '查看变更' })}
          </Button>
        ) : null}
        <Button size='small' data-testid='first-win-outcome-save' onClick={handleSaveFlow}>
          {t('conversation.firstWinOutcome.saveFlow', { defaultValue: '保存为流程' })}
        </Button>
        <Button type='text' size='small' data-testid='first-win-outcome-confirm' onClick={handleConfirmValue}>
          {t('conversation.firstWinOutcome.confirm', { defaultValue: '成果有用' })}
        </Button>
      </div>
    </section>
  );
};

export default FirstWinOutcomeCard;
