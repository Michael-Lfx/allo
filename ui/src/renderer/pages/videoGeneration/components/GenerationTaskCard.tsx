/**
 * Card component for displaying a generation task in the history list.
 * Supports video thumbnail preview with hover-to-play functionality.
 */
import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Tag } from '@arco-design/web-react';
import { Delete, VideoOne, Refresh, PlayOne } from '@icon-park/react';
import { canvasMediaUrl, type GenerationTaskView } from '../../videoCanvas/api';
import styles from '../index.module.css';

function formatRelativeTime(ms: number, t: ReturnType<typeof useTranslation>['t']): string {
  const diff = Date.now() - ms;
  const minutes = Math.floor(diff / 60000);
  const hours = Math.floor(minutes / 60);
  const days = Math.floor(hours / 24);
  if (minutes < 1) return t('videoGeneration.time.justNow', { defaultValue: '刚刚' });
  if (minutes < 60) return t('videoGeneration.time.minutesAgo', { count: minutes, defaultValue: '{{count}} 分钟前' });
  if (hours < 24) return t('videoGeneration.time.hoursAgo', { count: hours, defaultValue: '{{count}} 小时前' });
  if (days === 1) return t('videoGeneration.time.yesterday', { defaultValue: '昨天' });
  if (days < 7) return t('videoGeneration.time.daysAgo', { count: days, defaultValue: '{{count}} 天前' });
  return t('videoGeneration.time.weeksAgo', { defaultValue: '上周' });
}

function statusTagColor(status: string): string {
  switch (status) {
    case 'queued':
    case 'running':
      return 'arcoblue';
    case 'succeeded':
      return 'green';
    case 'failed':
      return 'red';
    case 'canceled':
      return 'orangered';
    default:
      return 'gray';
  }
}

function statusLabel(status: string, t: ReturnType<typeof useTranslation>['t']): string {
  return t(`videoGeneration.clip.status.${status}`, { defaultValue: status });
}

interface GenerationTaskCardProps {
  task: GenerationTaskView;
  onDelete?: (task: GenerationTaskView) => void;
  deleting?: boolean;
}

const GenerationTaskCard: React.FC<GenerationTaskCardProps> = ({ task, onDelete, deleting }) => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const cardRef = useRef<HTMLDivElement | null>(null);

  const [coverUrl, setCoverUrl] = useState<string | null>(null);
  const [videoUrl, setVideoUrl] = useState<string | null>(null);
  const [hovering, setHovering] = useState(false);
  const [loadVideo, setLoadVideo] = useState(false);
  const [visible, setVisible] = useState(false);

  const updatedMs = task.updated_at;
  const meta: string[] = [
    statusLabel(task.status, t),
    ...(updatedMs != null
      ? [
          t('videoGeneration.list.card.updatedAt', {
            time: formatRelativeTime(updatedMs * 1000, t),
            defaultValue: '{{time}} 更新',
          }),
        ]
      : []),
  ];

  // Lazy visibility observer
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

  // Load video thumbnail (use result_media_id as cover)
  useEffect(() => {
    let cancelled = false;
    if (!visible || !task.result_media_id) {
      setCoverUrl(null);
      return;
    }
    const url = canvasMediaUrl(task.result_media_id);
    if (!cancelled) setCoverUrl(url);
    return () => {
      cancelled = true;
    };
  }, [visible, task.result_media_id]);

  // Load video on hover
  useEffect(() => {
    let cancelled = false;
    if (!loadVideo || !task.result_media_id) {
      setVideoUrl(null);
      return;
    }
    const url = canvasMediaUrl(task.result_media_id);
    if (!cancelled) setVideoUrl(url);
    return () => {
      cancelled = true;
    };
  }, [loadVideo, task.result_media_id]);

  // Auto-play when hovering
  useEffect(() => {
    const el = videoRef.current;
    if (!hovering || !el || !videoUrl) return;
    void el.play().catch(() => {
      // autoplay may be blocked; ignore
    });
    return () => {
      if (el && !el.paused) {
        el.pause();
        el.currentTime = 0;
      }
    };
  }, [hovering, videoUrl]);

  const handleEnter = useCallback(() => {
    setHovering(true);
    setLoadVideo(true);
  }, []);

  const handleLeave = useCallback(() => {
    setHovering(false);
    const el = videoRef.current;
    if (!el) return;
    el.pause();
    el.currentTime = 0;
  }, []);

  const handleOpen = useCallback(() => {
    navigate(`/video-generation/clip/${encodeURIComponent(task.task_id)}`, {
      state: {
        title: task.prompt?.slice(0, 48) || t('videoGeneration.clip.defaultTitle'),
        prompt: task.prompt,
        taskId: task.task_id,
      },
    });
  }, [navigate, task.task_id, task.prompt, t]);

  const isSucceeded = task.status === 'succeeded' && task.result_media_id;

  return (
    <div
      ref={cardRef}
      role='button'
      tabIndex={0}
      className={[
        styles.projectCard,
        'group relative flex flex-col overflow-hidden box-border cursor-pointer',
      ].join(' ')}
      onClick={handleOpen}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          handleOpen();
        }
      }}
      onMouseEnter={handleEnter}
      onMouseLeave={handleLeave}
      onFocus={() => setLoadVideo(true)}
    >
      <div className={`${styles.projectCover} relative overflow-hidden`}>
        {/* Thumbnail image */}
        {coverUrl ? (
          <img
            src={coverUrl}
            alt=''
            className={[
              styles.projectCoverMedia,
              hovering && videoUrl ? styles.projectCoverHidden : '',
            ]
              .filter(Boolean)
              .join(' ')}
            draggable={false}
          />
        ) : null}

        {/* Hover video preview */}
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
              hovering ? styles.projectCoverVideoVisible : '',
            ]
              .filter(Boolean)
              .join(' ')}
          />
        ) : null}

        {/* Fallback placeholder */}
        {!coverUrl && !videoUrl ? (
          <div className={styles.projectCoverFallback}>
            <span className='flex h-28px w-28px items-center justify-center rd-8px border border-solid border-[rgba(var(--primary-6),0.2)] bg-[rgba(var(--primary-6),0.12)] text-[rgb(var(--primary-6))]'>
              <VideoOne theme='outline' size={15} fill='currentColor' />
            </span>
          </div>
        ) : null}

        {/* Status overlay */}
        <div className={styles.projectCoverOverlay}>
          <Tag size='small' color={statusTagColor(task.status)} className='shrink-0'>
            {statusLabel(task.status, t)}
          </Tag>
        </div>

        {/* Play indicator when hovering on succeeded videos */}
        {isSucceeded && hovering && (
          <div className={styles.projectCoverPlayIndicator}>
            <PlayOne theme='outline' size={32} fill='currentColor' />
          </div>
        )}
      </div>

      <div className='flex items-start gap-12px p-14px'>
        <div className='min-w-0 flex-1 flex flex-col gap-6px'>
          <div className='flex items-start justify-between gap-8px'>
            <div className='truncate text-15px font-600 leading-[1.3] text-[var(--color-text-1)]'>
              {task.prompt?.slice(0, 48) || t('videoGeneration.clip.defaultTitle')}
            </div>
            <div className='flex items-center gap-6px shrink-0'>
              {onDelete ? (
                <button
                  type='button'
                  aria-label={t('videoGeneration.actions.delete', { defaultValue: '删除' })}
                  title={t('videoGeneration.actions.delete', { defaultValue: '删除' })}
                  className={[
                    'inline-flex items-center justify-center w-24px h-24px rd-6px',
                    'text-[var(--color-text-3)] hover:text-[rgb(var(--danger-6))]',
                    'hover:bg-[var(--color-fill-2)] transition-colors',
                    deleting ? 'opacity-40 pointer-events-none' : '',
                  ].join(' ')}
                  onClick={(e) => {
                    e.stopPropagation();
                    onDelete(task);
                  }}
                  disabled={deleting}
                >
                  <Delete theme='outline' size={14} fill='currentColor' />
                </button>
              ) : (
                <button
                  type='button'
                  aria-label={t('videoGeneration.clip.regenerate', { defaultValue: '重新生成' })}
                  title={t('videoGeneration.clip.regenerate', { defaultValue: '重新生成' })}
                  className={[
                    'inline-flex items-center justify-center w-24px h-24px rd-6px',
                    'text-[var(--color-text-3)] hover:text-[rgb(var(--primary-6))]',
                    'hover:bg-[var(--color-fill-2)] transition-colors',
                  ].join(' ')}
                  onClick={(e) => {
                    e.stopPropagation();
                    handleOpen();
                  }}
                >
                  <Refresh theme='outline' size={14} fill='currentColor' />
                </button>
              )}
            </div>
          </div>
          {task.status === 'running' && task.progress != null && (
            <div className='text-12px text-[var(--color-text-3)]'>
              {t('videoGeneration.clip.progress', { percent: Math.round(task.progress) })}%
            </div>
          )}
          <div className='truncate text-11px text-[var(--color-text-4)]'>{meta.join(' · ')}</div>
        </div>
      </div>
    </div>
  );
};

export default React.memo(GenerationTaskCard);
