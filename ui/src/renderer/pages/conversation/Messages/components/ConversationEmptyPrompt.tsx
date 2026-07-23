

import React, { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { emitter } from '@/renderer/utils/emitter';
import { intentsForWorkspace } from '@/renderer/pages/guid/readiness/guidReadiness';

type ConversationEmptyPromptProps = {
  workspace?: string;
};

const ConversationEmptyPrompt: React.FC<ConversationEmptyPromptProps> = ({ workspace }) => {
  const { t } = useTranslation();
  const hasWorkspace = Boolean(workspace?.trim());

  const prompts = useMemo(() => {
    return intentsForWorkspace(hasWorkspace).map((intent) =>
      t(intent.textKey, { defaultValue: intent.defaultText })
    );
  }, [hasWorkspace, t]);

  return (
    <div className='w-full max-w-640px px-20px text-center' data-testid='conversation-empty-prompt'>
      <div className='text-16px font-medium text-t-primary mb-8px'>
        {t('conversation.emptyPrompt.title', { defaultValue: '从一个具体任务开始' })}
      </div>
      <div className='text-13px text-t-secondary mb-16px'>
        {t('conversation.emptyPrompt.subtitle', {
          defaultValue: '点击只填充输入框，由你决定何时发送。',
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
