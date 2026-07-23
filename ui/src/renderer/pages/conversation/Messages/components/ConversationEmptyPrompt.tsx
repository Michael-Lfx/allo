/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { emitter } from '@/renderer/utils/emitter';

type ConversationEmptyPromptProps = {
  workspace?: string;
};

const ConversationEmptyPrompt: React.FC<ConversationEmptyPromptProps> = ({ workspace }) => {
  const { t } = useTranslation();

  const prompts = useMemo(() => {
    const folderHint = workspace
      ? t('conversation.emptyPrompt.workspaceHint', {
          defaultValue: 'in this project',
        })
      : t('conversation.emptyPrompt.generalHint', { defaultValue: 'for this chat' });
    return [
      t('conversation.emptyPrompt.explore', {
        defaultValue: 'Summarize the current workspace {{hint}}',
        hint: folderHint,
      }),
      t('conversation.emptyPrompt.fix', {
        defaultValue: 'Find and fix the most likely bug {{hint}}',
        hint: folderHint,
      }),
      t('conversation.emptyPrompt.plan', {
        defaultValue: 'Propose a next-step plan {{hint}}',
        hint: folderHint,
      }),
    ];
  }, [t, workspace]);

  return (
    <div className='w-full max-w-640px px-20px text-center' data-testid='conversation-empty-prompt'>
      <div className='text-16px font-medium text-t-primary mb-8px'>
        {t('conversation.emptyPrompt.title', { defaultValue: 'Start with something concrete' })}
      </div>
      <div className='text-13px text-t-secondary mb-16px'>
        {t('conversation.emptyPrompt.subtitle', {
          defaultValue: 'These fill the composer only — you decide when to send.',
        })}
      </div>
      <div className='flex flex-col gap-8px'>
        {prompts.map((prompt) => (
          <button
            key={prompt}
            type='button'
            className='text-left px-12px py-10px rd-8px border-1 border-solid border-3 bg-base hover:bg-1 transition-colors text-13px text-t-primary'
            onClick={() => emitter.emit('sendbox.fill', prompt)}
          >
            {prompt}
          </button>
        ))}
      </div>
    </div>
  );
};

export default ConversationEmptyPrompt;
