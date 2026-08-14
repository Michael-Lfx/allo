import LoadingState from '@renderer/components/beautifulUi/loadingState/LoadingState';
import React from 'react';
import { useTranslation } from 'react-i18next';

/**
 * MessageListSkeleton — Beautiful UI loading chrome shown while a
 * conversation's message history is loading (or before the content page has
 * resolved its conversation). Conversation opening uses one language (`drive`);
 * Dots / Orbit stay on the isolated preview.
 */
const MessageListSkeleton: React.FC = () => {
  const { t } = useTranslation();

  return (
    <div
      className='flex-1 h-full overflow-y-auto pb-10px box-border'
      data-testid='message-list-skeleton'
      style={{ minHeight: '100%' }}
    >
      <div className='min-h-full flex items-center justify-center py-10px box-border'>
        <LoadingState variant='drive' label={t('conversation.skeleton.opening')} />
      </div>
    </div>
  );
};

export default MessageListSkeleton;
