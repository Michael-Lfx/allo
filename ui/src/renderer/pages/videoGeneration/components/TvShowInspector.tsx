import React, { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Modal, Spin } from '@arco-design/web-react';
import { Like } from '@icon-park/react';
import type { TvShowVideo } from '../types';
import { tvShowWorkflowLabel } from './SessionCard';

interface TvShowInspectorProps {
  video: TvShowVideo | null;
  loading?: boolean;
  importing?: boolean;
  liking?: boolean;
  authenticated: boolean;
  onClose: () => void;
  onImport: () => void;
  onToggleLike: () => void;
  onLogin: () => void;
  onPrev?: () => void;
  onNext?: () => void;
}

const TvShowInspector: React.FC<TvShowInspectorProps> = ({
  video,
  loading,
  importing,
  liking,
  authenticated,
  onClose,
  onImport,
  onToggleLike,
  onLogin,
  onPrev,
  onNext,
}) => {
  const { t } = useTranslation();

  useEffect(() => {
    if (!video) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'ArrowLeft') onPrev?.();
      if (event.key === 'ArrowRight') onNext?.();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onNext, onPrev, video]);

  return (
    <Modal
      visible={video != null}
      onCancel={onClose}
      footer={null}
      unmountOnExit
      style={{ width: 'min(920px, 94vw)' }}
      title={video?.title || t('videoGeneration.tvShow.detail.title', { defaultValue: '作品详情' })}
    >
      {loading && !video?.coverUrl ? (
        <div className='flex justify-center py-40px'>
          <Spin />
        </div>
      ) : video ? (
        <div className='flex flex-col gap-14px'>
          {video.previewUrl ? (
            <video
              key={video.previewUrl}
              src={video.previewUrl}
              poster={video.coverUrl || undefined}
              controls
              playsInline
              className='w-full rd-12px aspect-video bg-[#0c0f14] object-contain'
            />
          ) : video.coverUrl ? (
            <img
              src={video.coverUrl}
              alt=''
              className='w-full rd-12px aspect-video bg-[#0c0f14] object-contain'
            />
          ) : null}
          <div className='flex items-center gap-8px text-13px text-[var(--color-text-3)]'>
            {video.author?.avatarUrl ? (
              <img
                src={video.author.avatarUrl}
                alt=''
                className='h-22px w-22px rd-full object-cover'
              />
            ) : null}
            <span className='truncate'>
              {tvShowWorkflowLabel(video, t)}
              {video.author?.name ? ` · ${video.author.name}` : ''}
            </span>
          </div>
          {video.description ? (
            <p className='m-0 text-13px leading-[1.6] text-[var(--color-text-2)] whitespace-pre-wrap'>
              {video.description}
            </p>
          ) : null}
          {video.rejectReason && video.status === 'offline' ? (
            <div className='text-12px text-[rgb(var(--danger-6))]'>{video.rejectReason}</div>
          ) : null}
          <div className='flex flex-wrap gap-8px'>
            {video.status === 'published' ? (
              <Button
                type='outline'
                size='small'
                loading={liking}
                onClick={() => (authenticated ? onToggleLike() : onLogin())}
              >
                <span className='inline-flex items-center gap-4px'>
                  <Like theme={video.liked ? 'filled' : 'outline'} size={14} fill='currentColor' />
                  {video.likeCount ?? 0}
                </span>
              </Button>
            ) : null}
            <Button
              type='primary'
              size='small'
              loading={importing}
              disabled={authenticated && !video.packageUrl && loading}
              onClick={() => (authenticated ? onImport() : onLogin())}
            >
              {t('videoGeneration.tvShow.actions.remix', { defaultValue: '用这个做一支' })}
            </Button>
          </div>
        </div>
      ) : null}
    </Modal>
  );
};

export default TvShowInspector;
