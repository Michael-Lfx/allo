import { FolderClose, MessageOne } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';

type WorkpathHoverCardProps = {
  displayName: string;
  conversationCount: number;
  workspacePath?: string;
};

const WorkpathHoverCard: React.FC<WorkpathHoverCardProps> = ({ displayName, conversationCount, workspacePath }) => {
  const { t } = useTranslation();

  return (
    <div className='w-260px max-w-[calc(100vw-24px)] px-9px pt-9px pb-9px'>
      <div className='flex items-center gap-8px'>
        <span className='size-16px shrink-0 flex-center translate-y-1px'>
          <FolderClose theme='outline' size={14} fill='currentColor' className='block text-t-primary' />
        </span>
        <span className='min-w-0 truncate text-13px font-600 leading-18px text-t-primary'>{displayName}</span>
      </div>
      <div className='mt-8px flex items-center gap-8px'>
        <span className='size-16px shrink-0 flex-center translate-y-1px'>
          <MessageOne theme='outline' size={14} fill='currentColor' className='block text-t-secondary' />
        </span>
        <span className='text-13px font-400 leading-18px text-t-primary'>
          {t('sessionList.workpathConversationCount', { count: conversationCount })}
        </span>
      </div>
      {workspacePath && (
        <>
          <div className='my-8px h-px bg-[var(--color-border-2)]' />
          <div className='flex items-start gap-8px'>
            <span className='size-16px mt-0 shrink-0 flex-center translate-y-1px'>
              <FolderClose theme='outline' size={14} fill='currentColor' className='block text-t-secondary' />
            </span>
            <span className='min-w-0 break-all text-13px font-400 leading-18px text-t-primary'>{workspacePath}</span>
          </div>
        </>
      )}
    </div>
  );
};

export default WorkpathHoverCard;
