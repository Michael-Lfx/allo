import React, { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { Popconfirm, Spin, Tag } from '@arco-design/web-react';
import { Delete, VideoOne } from '@icon-park/react';
import {
  resolveBriefingUrl,
  type BriefingSessionSummary,
  type BriefingStatus,
} from '../briefing/api';
import { prefetchVideoBriefing } from '../prefetch';
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

function statusTagColor(status: string | null | undefined): string {
  switch (status) {
    case 'researching':
    case 'scripting':
    case 'aligning':
    case 'composing':
      return 'arcoblue';
    case 'succeeded':
      return 'green';
    case 'failed':
      return 'red';
    case 'cancelled':
    case 'interrupted':
      return 'orangered';
    case 'hold':
    case 'idle':
    default:
      return 'gray';
  }
}

function statusLabel(status: string | null | undefined, t: TFunction): string {
  const key = (status ?? 'idle') as BriefingStatus | string;
  return t(`videoGeneration.briefing.status.${key}`, { defaultValue: key });
}

interface BriefingCardProps {
  briefing: BriefingSessionSummary;
  onOpen: (session: BriefingSessionSummary) => void;
  onDelete?: (session: BriefingSessionSummary) => void;
  deleting?: boolean;
  opening?: boolean;
  disabled?: boolean;
}

const BriefingCard: React.FC<BriefingCardProps> = ({
  briefing,
  onOpen,
  onDelete,
  deleting,
  opening,
  disabled,
}) => {
  const { t } = useTranslation();
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const cardRef = useRef<HTMLDivElement | null>(null);
  const [hovering, setHovering] = useState(false);
  const [loadVideo, setLoadVideo] = useState(false);
  const [visible, setVisible] = useState(false);

  const videoRel = loadVideo ? briefing.final_video?.trim() || null : null;
  const videoUrl = videoRel ? resolveBriefingUrl(briefing.id, videoRel) : null;
  const updatedMs = toEpochMs(briefing.updated_at ?? briefing.created_at);
  const kindLabel = t('videoGeneration.list.briefingKind', {
    defaultValue: '资讯口播',
  });
  const meta: string[] = [
    kindLabel,
    ...(updatedMs != null
      ? [
          t('videoGeneration.list.card.updatedAt', {
            time: formatRelativeTime(updatedMs, t),
            defaultValue: '{{time}} 更新',
          }),
        ]
      : []),
  ];

  useEffect(() => {
    const element = cardRef.current;
    if (!element || typeof IntersectionObserver === 'undefined') {
      setVisible(true);
      return;
    }
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (!entry?.isIntersecting) return;
        setVisible(true);
        observer.disconnect();
      },
      { rootMargin: '200px' }
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const handleEnter = () => {
    prefetchVideoBriefing();
    setHovering(true);
    if (visible) setLoadVideo(true);
    const el = videoRef.current;
    if (!el || !videoUrl) return;
    void el.play().catch(() => {
      /* autoplay may be blocked; ignore */
    });
  };

  const handleLeave = () => {
    setHovering(false);
    const el = videoRef.current;
    if (!el) return;
    el.pause();
  };

  useEffect(() => {
    if (visible && briefing.final_video) setLoadVideo(true);
  }, [visible, briefing.final_video]);

  useEffect(() => {
    const element = videoRef.current;
    if (!hovering || !element || !videoUrl) return;
    void element.play().catch(() => {
      /* autoplay may be blocked; ignore */
    });
  }, [hovering, videoUrl]);

  return (
    <div
      ref={cardRef}
      role='button'
      tabIndex={0}
      data-testid='briefing-card'
      className={[
        styles.projectCard,
        'group relative flex flex-col overflow-hidden box-border cursor-pointer',
        disabled && 'opacity-60 pointer-events-none',
      ]
        .filter(Boolean)
        .join(' ')}
      onClick={() => {
        if (disabled || opening || deleting) return;
        onOpen(briefing);
      }}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          if (disabled || opening || deleting) return;
          e.preventDefault();
          onOpen(briefing);
        }
      }}
      onMouseEnter={handleEnter}
      onMouseLeave={handleLeave}
      onFocus={() => setLoadVideo(true)}
      onPointerEnter={() => prefetchVideoBriefing()}
    >
      {opening ? (
        <div className='absolute inset-0 bg-[var(--color-bg-1)]/60 backdrop-blur-xs flex items-center justify-center z-10'>
          <Spin size={20} />
        </div>
      ) : null}
      <div className={`${styles.projectCover} relative overflow-hidden`}>
        {videoUrl ? (
          <video
            ref={videoRef}
            src={videoUrl}
            muted
            playsInline
            loop
            preload='metadata'
            className={[
              styles.projectCoverMedia,
              styles.projectCoverVideo,
              hovering || briefing.status === 'succeeded' ? styles.projectCoverVideoVisible : '',
            ]
              .filter(Boolean)
              .join(' ')}
            onLoadedMetadata={(event) => {
              const el = event.currentTarget;
              if (el.duration > 0.15 && el.currentTime < 0.05) el.currentTime = 0.08;
            }}
          />
        ) : (
          <div className={styles.projectCoverFallback}>
            <span className='flex h-28px w-28px items-center justify-center rd-8px border border-solid border-[rgba(var(--primary-6),0.2)] bg-[rgba(var(--primary-6),0.12)] text-[rgb(var(--primary-6))]'>
              <VideoOne theme='outline' size={15} fill='currentColor' />
            </span>
          </div>
        )}
        <div className={styles.projectCoverOverlay}>
          <Tag size='small' color={statusTagColor(briefing.status)} className='shrink-0'>
            {statusLabel(briefing.status, t)}
          </Tag>
        </div>
      </div>

      <div className='flex items-start gap-12px p-14px'>
        <div className='min-w-0 flex-1 flex flex-col gap-6px'>
          <div className='flex items-start justify-between gap-8px'>
            <div className='truncate text-15px font-600 leading-[1.3] text-[var(--color-text-1)]'>
              {briefing.title || t('videoGeneration.list.untitled', { defaultValue: '未命名任务' })}
            </div>
            <div className='flex items-center gap-6px shrink-0'>
              {onDelete ? (
                <Popconfirm
                  title={t('videoGeneration.actions.deleteConfirm', {
                    defaultValue: '确定删除该任务？产物将一并清除。',
                  })}
                  disabled={deleting}
                  onOk={(e) => {
                    e?.stopPropagation?.();
                    onDelete(briefing);
                  }}
                >
                  <span
                    role='button'
                    tabIndex={0}
                    data-testid='briefing-card-delete'
                    aria-label={t('videoGeneration.actions.delete', { defaultValue: '删除' })}
                    title={t('videoGeneration.actions.delete', { defaultValue: '删除' })}
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
          {briefing.stage ? (
            <div className='truncate text-12px text-[var(--color-text-3)]'>
              {t(`videoGeneration.briefing.stage.${briefing.stage}`, {
                defaultValue: briefing.stage,
              })}
            </div>
          ) : null}
          <div className='truncate text-11px text-[var(--color-text-4)]'>{meta.join(' · ')}</div>
        </div>
      </div>
    </div>
  );
};

export default React.memo(BriefingCard);
