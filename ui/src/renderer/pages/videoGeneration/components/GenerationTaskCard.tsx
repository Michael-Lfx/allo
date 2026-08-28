/**
 * Card component for displaying a generation task in the history list.
 * Succeeded clips show a muted looping video preview instead of a poster image
 * (the media endpoint serves MP4, which cannot be used as `<img src>`).
 */
import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Popconfirm, Tag } from '@arco-design/web-react';
import { Delete, VideoOne, LoadingOne } from '@icon-park/react';
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

  const [inView, setInView] = useState(false);
  const [previewReady, setPreviewReady] = useState(false);

  const isSucceeded = task.status === 'succeeded' && Boolean(task.result_media_id);
  const videoUrl = isSucceeded && inView && task.result_media_id
    ? canvasMediaUrl(task.result_media_id)
    : null;

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

  useEffect(() => {
    const element = cardRef.current;
    if (!element || typeof IntersectionObserver === 'undefined') {
      setInView(true);
      return;
    }
    const observer = new IntersectionObserver(
      ([entry]) => {
        setInView(Boolean(entry?.isIntersecting));
      },
      { rootMargin: '120px', threshold: 0.15 }
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    setPreviewReady(false);
  }, [videoUrl]);

  useEffect(() => {
    const el = videoRef.current;
    if (!el || !videoUrl || !inView) return;
    el.muted = true;
    const play = () => {
      void el.play().catch(() => {
        // Autoplay can still fail; the first decoded frame remains visible.
      });
    };
    if (el.readyState >= 2) {
      play();
    } else {
      el.addEventListener('canplay', play, { once: true });
    }
    return () => {
      el.removeEventListener('canplay', play);
      if (!el.paused) el.pause();
    };
  }, [videoUrl, inView]);

  const handleOpen = useCallback(() => {
    navigate(`/video-generation/clip/${encodeURIComponent(task.task_id)}`, {
      state: {
        title: task.prompt?.slice(0, 48) || t('videoGeneration.clip.defaultTitle'),
        prompt: task.prompt,
        taskId: task.task_id,
      },
    });
  }, [navigate, task.task_id, task.prompt, t]);

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
    >
      <div className={`${styles.projectCover} relative overflow-hidden`}>
        {videoUrl ? (
          <video
            ref={videoRef}
            src={videoUrl}
            muted
            playsInline
            loop
            autoPlay
            preload='auto'
            className={`${styles.projectCoverMedia} ${styles.projectCoverVideo}`}
            onLoadedData={() => setPreviewReady(true)}
            onLoadedMetadata={(event) => {
              const el = event.currentTarget;
              if (el.duration > 0.15 && el.currentTime < 0.05) {
                el.currentTime = 0.08;
              }
            }}
          />
        ) : null}

        {(!videoUrl || !previewReady) && (
          <div className={styles.projectCoverFallback}>
            <span className='flex h-28px w-28px items-center justify-center rd-8px border border-solid border-[rgba(var(--primary-6),0.2)] bg-[rgba(var(--primary-6),0.12)] text-[rgb(var(--primary-6))]'>
              {task.status === 'queued' || task.status === 'running' ? (
                <LoadingOne theme='outline' size={15} fill='currentColor' className='animate-spin' />
              ) : (
                <VideoOne theme='outline' size={15} fill='currentColor' />
              )}
            </span>
          </div>
        )}

        <div className={styles.projectCoverOverlay}>
          <Tag size='small' color={statusTagColor(task.status)} className='shrink-0'>
            {statusLabel(task.status, t)}
          </Tag>
        </div>
      </div>

      <div className='flex items-start gap-12px p-14px'>
        <div className='min-w-0 flex-1 flex flex-col gap-6px'>
          <div className='flex items-start justify-between gap-8px'>
            <div className='truncate text-15px font-600 leading-[1.3] text-[var(--color-text-1)]'>
              {task.prompt?.slice(0, 48) || t('videoGeneration.clip.defaultTitle')}
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
                    onDelete(task);
                  }}
                >
                  <span
                    role='button'
                    tabIndex={0}
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
