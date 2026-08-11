import React from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { Popconfirm, Tag } from '@arco-design/web-react';
import { Delete, VideoOne } from '@icon-park/react';
import type { MontageRunStatus, ProjectSummary, VideoGenMode } from '../types';
import { stageLabel } from '../stageI18n';
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

export function modeLabel(mode: VideoGenMode | string | null | undefined, t: TFunction): string {
  switch (mode) {
    case 'avatar':
      return t('videoGeneration.mode.avatarLabel', { defaultValue: '数字人' });
    case 'talking_head':
      return t('videoGeneration.mode.avatarLabel', { defaultValue: '数字人' });
    case 'creation':
      return t('videoGeneration.mode.creationLabel', { defaultValue: '创作模式' });
    case 'agent':
    default:
      return t('videoGeneration.mode.agentLabel', { defaultValue: 'Agent 模式' });
  }
}

export function pipelineLabel(pipeline: string | null | undefined, t: TFunction): string {
  const name = (pipeline ?? '').trim();
  if (!name) return t('videoGeneration.list.untitled', { defaultValue: '未命名任务' });
  const key = `videoGeneration.pipeline.${name}.title`;
  const translated = t(key, { defaultValue: '' });
  return translated || name;
}

export function pipelineDescription(
  pipeline: string | null | undefined,
  t: TFunction,
  fallback = ''
): string {
  const name = (pipeline ?? '').trim();
  if (!name) return fallback;
  const key = `videoGeneration.pipeline.${name}.desc`;
  const translated = t(key, { defaultValue: '' });
  return translated || fallback;
}

export function statusTagColor(status: MontageRunStatus | string | null | undefined): string {
  switch (status) {
    case 'running':
      return 'arcoblue';
    case 'awaiting_human':
      return 'orangered';
    case 'succeeded':
      return 'green';
    case 'failed':
      return 'red';
    case 'cancelled':
      return 'gray';
    case 'idle':
    default:
      return 'gray';
  }
}

export function statusLabel(status: MontageRunStatus | string | null | undefined, t: TFunction): string {
  const key = status ?? 'idle';
  return t(`videoGeneration.status.${key}`, { defaultValue: String(key) });
}

interface SessionCardProps {
  session: ProjectSummary;
  onOpen: (s: ProjectSummary) => void;
  onDelete?: (s: ProjectSummary) => void;
  deleting?: boolean;
}

const SessionCard: React.FC<SessionCardProps> = ({ session, onOpen, onDelete, deleting }) => {
  const { t } = useTranslation();

  const updatedMs = toEpochMs(session.updated_at ?? session.created_at);
  const meta: string[] = [
    pipelineLabel(session.pipeline, t),
    modeLabel(session.mode, t),
    ...(updatedMs != null
      ? [
          t('videoGeneration.list.card.updatedAt', {
            time: formatRelativeTime(updatedMs, t),
            defaultValue: '{{time}} 更新',
          }),
        ]
      : []),
  ];

  return (
    <div
      role='button'
      tabIndex={0}
      className={[
        styles.projectCard,
        'group relative flex flex-col overflow-hidden box-border cursor-pointer',
      ].join(' ')}
      onClick={() => onOpen(session)}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          onOpen(session);
        }
      }}
    >
      <div className={`${styles.projectCover} relative overflow-hidden`}>
        <div className={styles.projectCoverFallback}>
          <span className='flex h-28px w-28px items-center justify-center rd-8px border border-solid border-[rgba(var(--primary-6),0.2)] bg-[rgba(var(--primary-6),0.12)] text-[rgb(var(--primary-6))]'>
            <VideoOne theme='outline' size={15} fill='currentColor' />
          </span>
        </div>
        <div className={styles.projectCoverOverlay}>
          <Tag size='small' color={statusTagColor(session.status)} className='shrink-0'>
            {statusLabel(session.status, t)}
          </Tag>
          {session.final_video ? (
            <Tag size='small' color='green' className='shrink-0'>
              {t('videoGeneration.list.card.hasFinal', { defaultValue: '可预览' })}
            </Tag>
          ) : null}
        </div>
      </div>

      <div className='flex items-start gap-12px p-14px'>
        <div className='min-w-0 flex-1 flex flex-col gap-6px'>
          <div className='flex items-start justify-between gap-8px'>
            <div className='truncate text-15px font-600 leading-[1.3] text-[var(--color-text-1)]'>
              {session.title || t('videoGeneration.list.untitled', { defaultValue: '未命名任务' })}
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
                    onDelete(session);
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
          {session.current_stage ? (
            <div className='truncate text-12px text-[var(--color-text-3)]'>
              {stageLabel(session.current_stage, t)}
            </div>
          ) : null}
          <div className='truncate text-11px text-[var(--color-text-4)]'>{meta.join(' · ')}</div>
        </div>
      </div>
    </div>
  );
};

export default SessionCard;
