/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { Tag } from '@arco-design/web-react';
import { VideoOne } from '@icon-park/react';
import type { SessionSummary, VimaxRunStatus, VimaxWorkflow } from '../types';

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

/** Normalize API workflow ids (`novel2_video` → `novel2video`). */
export function normalizeWorkflow(workflow: string | null | undefined): VimaxWorkflow {
  const raw = (workflow ?? '').trim().toLowerCase().replace(/_/g, '');
  if (raw === 'script2video' || raw === 'script') return 'script2video';
  if (raw === 'novel2video' || raw === 'novel' || raw === 'novel2movie') return 'novel2video';
  return 'idea2video';
}

export function workflowLabel(workflow: VimaxWorkflow | string, t: TFunction): string {
  switch (normalizeWorkflow(workflow)) {
    case 'idea2video':
      return t('videoGeneration.workflow.idea2video.title', { defaultValue: '灵感成片' });
    case 'script2video':
      return t('videoGeneration.workflow.script2video.title', { defaultValue: '剧本成片' });
    case 'novel2video':
      return t('videoGeneration.workflow.novel2video.title', { defaultValue: '小说成片' });
    default:
      return String(workflow);
  }
}

export function statusTagColor(status: VimaxRunStatus | null | undefined): string {
  switch (status) {
    case 'planning':
    case 'rendering':
      return 'arcoblue';
    case 'succeeded':
      return 'green';
    case 'failed':
      return 'red';
    case 'cancelled':
      return 'orangered';
    case 'idle':
    default:
      return 'gray';
  }
}

export function statusLabel(status: VimaxRunStatus | null | undefined, t: TFunction): string {
  const key = status ?? 'idle';
  return t(`videoGeneration.status.${key}`, { defaultValue: key });
}

interface SessionCardProps {
  session: SessionSummary;
  onOpen: (s: SessionSummary) => void;
}

const SessionCard: React.FC<SessionCardProps> = ({ session, onOpen }) => {
  const { t } = useTranslation();
  const updatedMs = toEpochMs(session.updated_at ?? session.created_at);
  const meta: string[] = [
    workflowLabel(session.workflow, t),
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
        'group relative flex flex-col overflow-hidden rd-12px border border-solid',
        'border-[var(--color-border-2)] bg-[var(--color-bg-2)] box-border cursor-pointer',
        'transition-all duration-160',
        'hover:border-[var(--color-border-3)] hover:bg-[var(--color-fill-1)]',
      ].join(' ')}
      onClick={() => onOpen(session)}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          onOpen(session);
        }
      }}
    >
      <div className='flex items-start gap-12px p-16px'>
        <span
          className='flex items-center justify-center w-40px h-40px rd-10px shrink-0 text-[rgb(var(--primary-6))]'
          style={{
            background: 'rgba(var(--primary-6),0.1)',
            border: '1px solid rgba(var(--primary-6),0.18)',
          }}
        >
          <VideoOne theme='outline' size={20} fill='currentColor' className='block' style={{ lineHeight: 0 }} />
        </span>

        <div className='min-w-0 flex-1 flex flex-col gap-6px'>
          <div className='flex items-start justify-between gap-8px'>
            <div className='truncate text-15px font-600 leading-[1.3] text-[var(--color-text-1)]'>
              {session.title || t('videoGeneration.list.untitled', { defaultValue: '未命名任务' })}
            </div>
            <Tag size='small' color={statusTagColor(session.status)} className='shrink-0'>
              {statusLabel(session.status, t)}
            </Tag>
          </div>

          {session.stage ? (
            <div className='text-12px text-[var(--color-text-3)] truncate'>
              {t('videoGeneration.list.card.stage', {
                stage: session.stage,
                defaultValue: '阶段：{{stage}}',
              })}
            </div>
          ) : null}

          <div className='flex flex-wrap items-center gap-7px text-12px leading-16px text-[var(--color-text-3)]'>
            {meta.map((item, index) => (
              <React.Fragment key={item}>
                {index > 0 && (
                  <i className='h-3px w-3px rounded-full bg-[var(--color-fill-4)]' aria-hidden='true' />
                )}
                <span className='whitespace-nowrap'>{item}</span>
              </React.Fragment>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
};

export default SessionCard;
