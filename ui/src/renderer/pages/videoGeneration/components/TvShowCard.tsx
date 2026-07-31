
import React, { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { Popconfirm, Tag } from '@arco-design/web-react';
import { Delete, Like, VideoOne } from '@icon-park/react';
import type { TvShowStatus, TvShowVideo } from '../types';
import { normalizeWorkflow, workflowLabel } from './SessionCard';
import styles from '../index.module.css';

function toEpochMs(value: string | number | null | undefined): number | null {
  if (value == null) return null;
  if (typeof value === 'number' && Number.isFinite(value)) {
    return value < 1e12 ? value * 1000 : value;
  }
  const parsed = Date.parse(String(value));
  return Number.isNaN(parsed) ? null : parsed;
}

function formatRelativeTime(epochMs: number, t: TFunction): string {
  const diff = Date.now() - epochMs;
  const minutes = Math.floor(diff / 60000);
  const hours = Math.floor(minutes / 60);
  const days = Math.floor(hours / 24);
  if (minutes < 1) return t('videoGeneration.time.justNow', { defaultValue: '刚刚' });
  if (minutes < 60)
    return t('videoGeneration.time.minutesAgo', { count: minutes, defaultValue: '{{count}} 分钟前' });
  if (hours < 24)
    return t('videoGeneration.time.hoursAgo', { count: hours, defaultValue: '{{count}} 小时前' });
  if (days === 1) return t('videoGeneration.time.yesterday', { defaultValue: '昨天' });
  if (days < 7)
    return t('videoGeneration.time.daysAgo', { count: days, defaultValue: '{{count}} 天前' });
  return t('videoGeneration.time.weeksAgo', { defaultValue: '上周' });
}

function statusTagColor(status: TvShowStatus | string | null | undefined): string {
  switch (status) {
    case 'published':
      return 'green';
    case 'pending':
      return 'arcoblue';
    case 'offline':
      return 'orangered';
    case 'deleted':
      return 'gray';
    default:
      return 'gray';
  }
}

function statusLabel(status: TvShowStatus | string | null | undefined, t: TFunction): string {
  const key = status || 'pending';
  return t(`videoGeneration.tvShow.status.${key}`, { defaultValue: key });
}

interface TvShowCardProps {
  video: TvShowVideo;
  onOpen: (video: TvShowVideo) => void;
  onToggleLike?: (video: TvShowVideo) => void;
  onDelete?: (video: TvShowVideo) => void;
  liking?: boolean;
  deleting?: boolean;
  /** Show publish-status tag (mine list) instead of only like count. */
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
  const timeMs = toEpochMs(video.publishedAt ?? video.submittedAt ?? video.updatedAt);
  const meta = useMemo(() => {
    const parts = [
      workflowLabel(normalizeWorkflow(String(video.workflow)), t),
      video.author?.name?.trim() || null,
      ...(timeMs != null
        ? [
            t('videoGeneration.tvShow.card.publishedAt', {
              time: formatRelativeTime(timeMs, t),
              defaultValue: '{{time}}',
            }),
          ]
        : []),
    ].filter(Boolean) as string[];
    return parts.join(' · ');
  }, [t, timeMs, video.author?.name, video.workflow]);

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
    >
      <div className={`${styles.projectCover} relative overflow-hidden`}>
        {video.coverUrl ? (
          <img
            src={video.coverUrl}
            alt=''
            className={styles.projectCoverMedia}
            draggable={false}
          />
        ) : (
          <div className={styles.projectCoverFallback}>
            <span className='flex h-28px w-28px items-center justify-center rd-8px border border-solid border-[rgba(var(--primary-6),0.2)] bg-[rgba(var(--primary-6),0.12)] text-[rgb(var(--primary-6))]'>
              <VideoOne theme='outline' size={15} fill='currentColor' />
            </span>
          </div>
        )}
        <div className={styles.projectCoverOverlay}>
          {showStatus ? (
            <Tag size='small' color={statusTagColor(video.status)} className='shrink-0'>
              {statusLabel(video.status, t)}
            </Tag>
          ) : (
            <Tag size='small' color='arcoblue' className='shrink-0'>
              {workflowLabel(normalizeWorkflow(String(video.workflow)), t)}
            </Tag>
          )}
        </div>
      </div>

      <div className='flex items-start gap-12px p-14px'>
        <div className='min-w-0 flex-1 flex flex-col gap-6px'>
          <div className='flex items-start justify-between gap-8px'>
            <div className='truncate text-15px font-600 leading-[1.3] text-[var(--color-text-1)]'>
              {video.title || t('videoGeneration.list.untitled', { defaultValue: '未命名任务' })}
            </div>
            <div className='flex items-center gap-6px shrink-0'>
              {onToggleLike && video.status === 'published' ? (
                <span
                  role='button'
                  tabIndex={0}
                  aria-label={t('videoGeneration.tvShow.actions.like', { defaultValue: '点赞' })}
                  title={t('videoGeneration.tvShow.actions.like', { defaultValue: '点赞' })}
                  className={[
                    'inline-flex items-center gap-3px h-24px px-6px rd-6px text-11px',
                    video.liked
                      ? 'text-[rgb(var(--danger-6))]'
                      : 'text-[var(--color-text-3)] hover:text-[var(--color-text-1)]',
                    'hover:bg-[var(--color-fill-2)] transition-colors',
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
                      'inline-flex items-center justify-center w-24px h-24px rd-6px',
                      'text-[var(--color-text-3)] hover:text-[rgb(var(--danger-6))]',
                      'hover:bg-[var(--color-fill-2)] transition-colors',
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
          {video.rejectReason && video.status === 'offline' ? (
            <div className='truncate text-12px text-[rgb(var(--danger-6))]'>
              {video.rejectReason}
            </div>
          ) : null}
          <div className='truncate text-11px text-[var(--color-text-4)]'>{meta}</div>
        </div>
      </div>
    </div>
  );
};

export default TvShowCard;
