
import React, { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Popconfirm } from '@arco-design/web-react';
import { Delete, Like, VideoOne } from '@icon-park/react';
import type { TvShowVideo } from '../types';
import styles from '../index.module.css';

interface TvShowCardProps {
  video: TvShowVideo;
  onOpen: (video: TvShowVideo) => void;
  onToggleLike?: (video: TvShowVideo) => void;
  onDelete?: (video: TvShowVideo) => void;
  liking?: boolean;
  deleting?: boolean;
  showStatus?: boolean;
}

const TvShowCard: React.FC<TvShowCardProps> = ({
  video,
  onOpen,
  onToggleLike,
  onDelete,
  liking,
  deleting,
  showStatus,
}) => {
  const { t } = useTranslation();
  const videoRef = useRef<HTMLVideoElement>(null);
  const [hovering, setHovering] = useState(false);
  const [loadVideo, setLoadVideo] = useState(false);
  const previewUrl = video.previewUrl?.trim() || '';
  const awardText = video.awardLabel || (video.awardLevel
    ? t(`videoGeneration.campaign.award.${video.awardLevel}`, { defaultValue: video.awardLevel })
    : '');
  const pill = showStatus
    ? t(`videoGeneration.tvShow.status.${video.status || 'pending'}`, {
        defaultValue: video.status || 'pending',
      })
    : awardText;

  const handleEnter = () => {
    setHovering(true);
    if (previewUrl) setLoadVideo(true);
    void videoRef.current?.play().catch(() => undefined);
  };

  const handleLeave = () => {
    setHovering(false);
    const el = videoRef.current;
    if (!el) return;
    el.pause();
    window.setTimeout(() => {
      const node = videoRef.current;
      if (node && node.paused) {
        node.currentTime = 0;
      }
    }, 180);
  };

  useEffect(() => {
    if (!hovering || !previewUrl) return;
    void videoRef.current?.play().catch(() => undefined);
  }, [hovering, previewUrl]);

  return (
    <div
      role='button'
      tabIndex={0}
      className={[
        styles.projectCard,
        'group relative flex flex-col overflow-hidden box-border cursor-pointer',
      ].join(' ')}
      onClick={() => onOpen(video)}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          onOpen(video);
        }
      }}
      onMouseEnter={handleEnter}
      onMouseLeave={handleLeave}
      onFocus={() => {
        if (previewUrl) setLoadVideo(true);
      }}
    >
      <div className={`${styles.tvLockup} overflow-hidden`}>
        {video.coverUrl ? (
          <img
            src={video.coverUrl}
            alt=''
            className={[
              styles.tvLockupMedia,
              hovering && previewUrl ? styles.projectCoverHidden : '',
            ]
              .filter(Boolean)
              .join(' ')}
            draggable={false}
            loading='lazy'
            decoding='async'
          />
        ) : (
          <div className={styles.projectCoverFallback}>
            <span className='flex h-28px w-28px items-center justify-center rd-8px border border-solid border-[rgba(var(--primary-6),0.2)] bg-[rgba(var(--primary-6),0.12)] text-[rgb(var(--primary-6))]'>
              <VideoOne theme='outline' size={15} fill='currentColor' />
            </span>
          </div>
        )}
        {previewUrl && loadVideo ? (
          <video
            ref={videoRef}
            src={previewUrl}
            poster={video.coverUrl || undefined}
            muted
            playsInline
            loop
            preload='metadata'
            className={[
              styles.tvLockupMedia,
              styles.projectCoverVideo,
              hovering ? styles.projectCoverVideoVisible : '',
            ].join(' ')}
          />
        ) : null}
        {pill ? <span className={styles.tvLockupPill}>{pill}</span> : null}
        <div className={styles.tvLockupScrim}>
          {video.author?.avatarUrl ? (
            <img
              src={video.author.avatarUrl}
              alt=''
              className='h-22px w-22px shrink-0 rd-full object-cover'
            />
          ) : null}
          <div className='min-w-0 truncate text-13px font-600 leading-[1.3]'>
            {video.title || t('videoGeneration.list.untitled', { defaultValue: '未命名任务' })}
          </div>
        </div>
        <div className={styles.tvLockupActions}>
          {onToggleLike && video.status === 'published' ? (
            <span
              role='button'
              tabIndex={0}
              aria-label={t('videoGeneration.tvShow.actions.like', { defaultValue: '点赞' })}
              className={[
                'inline-flex items-center gap-3px h-24px px-6px rd-999px text-11px text-white bg-[rgba(0,0,0,0.55)]',
                liking ? 'opacity-40 pointer-events-none' : '',
              ].join(' ')}
              onClick={(e) => {
                e.stopPropagation();
                onToggleLike(video);
              }}
              onKeyDown={(e) => {
                e.stopPropagation();
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  onToggleLike(video);
                }
              }}
            >
              <Like theme={video.liked ? 'filled' : 'outline'} size={14} fill='currentColor' />
              {video.likeCount ?? 0}
            </span>
          ) : null}
          {onDelete && video.isMine ? (
            <Popconfirm
              title={t('videoGeneration.tvShow.actions.deleteConfirm', {
                defaultValue: '确定删除该发布？',
              })}
              disabled={deleting}
              onOk={(e) => {
                e?.stopPropagation?.();
                onDelete(video);
              }}
            >
              <span
                role='button'
                tabIndex={0}
                aria-label={t('videoGeneration.actions.delete', { defaultValue: '删除' })}
                className={[
                  'inline-flex items-center justify-center w-24px h-24px rd-full text-white bg-[rgba(0,0,0,0.55)]',
                  deleting ? 'opacity-40 pointer-events-none' : '',
                ].join(' ')}
                onClick={(e) => e.stopPropagation()}
                onKeyDown={(e) => {
                  e.stopPropagation();
                  if (e.key === ' ') {
                    e.preventDefault();
                    (e.currentTarget as HTMLElement).click();
                  }
                }}
              >
                <Delete theme='outline' size={14} fill='currentColor' />
              </span>
            </Popconfirm>
          ) : null}
        </div>
      </div>
    </div>
  );
};

export default React.memo(TvShowCard);
