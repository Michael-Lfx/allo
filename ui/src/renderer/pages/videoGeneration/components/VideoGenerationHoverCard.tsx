/**
 * Hover popover for a recent video-generation project — mirrors ConversationHoverCard
 * so sider session rows and video project rows share the same soft card surface.
 */

import React from 'react';
import { useTranslation } from 'react-i18next';
import classNames from 'classnames';
import CopyIconButton from '@/renderer/components/base/CopyIconButton';
import { isActiveStatus } from '@renderer/pages/videoGeneration/api';
import { statusLabel } from '@renderer/pages/videoGeneration/components/SessionCard';
import type { VimaxRunStatus } from '@renderer/pages/videoGeneration/types';

export type VideoGenerationHoverCardProps = {
  id: string;
  title: string;
  status?: string | null;
};

const statusDotClass = (status: string | null | undefined) => {
  if (status === 'planning' || status === 'rendering') return 'bg-[rgb(var(--primary-6))]';
  if (status === 'failed') return 'bg-red-500';
  if (status === 'succeeded') return 'bg-green-500';
  return 'bg-t-tertiary';
};

const VideoGenerationHoverCard: React.FC<VideoGenerationHoverCardProps> = ({
  id,
  title,
  status,
}) => {
  const { t } = useTranslation();
  const displayTitle =
    title.trim() || t('videoGeneration.list.untitled', { defaultValue: '未命名任务' });
  const runStatus = (status ?? 'idle') as VimaxRunStatus;
  const busy = isActiveStatus(runStatus);

  return (
    <div className='flex flex-col gap-6px py-4px min-w-200px max-w-320px'>
      <div className='flex flex-col gap-2px min-w-0'>
        <span className='text-12px text-t-tertiary'>
          {t('videoGeneration.nav.hoverCard.name', { defaultValue: '项目名' })}
        </span>
        <span
          className='text-13px text-t-primary break-words line-clamp-3'
          style={{ wordBreak: 'break-word', overflowWrap: 'anywhere' }}
        >
          {displayTitle}
        </span>
      </div>

      <div className='flex flex-col gap-2px min-w-0'>
        <span className='text-12px text-t-tertiary'>
          {t('videoGeneration.nav.hoverCard.id', { defaultValue: '项目 ID' })}
        </span>
        <div className='flex items-center gap-6px min-w-0'>
          <span
            className='text-12px text-t-primary font-mono leading-16px flex-1 min-w-0 break-all'
          >
            {id}
          </span>
          <CopyIconButton
            text={id}
            tooltip={t('common.copyFullId', { defaultValue: '复制完整 ID' })}
            className='shrink-0 size-18px'
          />
        </div>
      </div>

      <div className='flex flex-col gap-2px'>
        <span className='text-12px text-t-tertiary'>
          {t('videoGeneration.nav.hoverCard.status', { defaultValue: '状态' })}
        </span>
        <span className='flex items-center gap-6px text-13px text-t-primary'>
          <span
            className={classNames(
              'size-6px rd-full shrink-0',
              statusDotClass(runStatus),
              busy && 'animate-pulse'
            )}
          />
          {statusLabel(runStatus, t)}
        </span>
      </div>
    </div>
  );
};

export default VideoGenerationHoverCard;
