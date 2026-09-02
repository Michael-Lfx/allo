

import React, { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Progress, Button, Tag, Spin } from '@arco-design/web-react';
import { classifyFailure } from '../classifyFailure';
import { eventElapsed, formatElapsedClock, coalesceProgressEvents } from '../progressEventElapsed';
import { statusLabel, statusTagColor } from './SessionCard';
import { stageLabel } from '../stageI18n';
import { useDocumentHidden, useRunStatusFull } from '../useRunStatusFeed';

interface ProgressTimelineProps {
  onCancel?: () => void;
  cancelling?: boolean;
  /** Currently selected models — used to explain failures. */
  models?: {
    llm_model?: string;
    image_model?: string;
    video_model?: string;
  };
  /** Live aggregate of Flowy video-task credits for this session. */
  creditsConsumed?: number;
}

const ProgressTimeline: React.FC<ProgressTimelineProps> = ({
  onCancel,
  cancelling,
  models,
  creditsConsumed,
}) => {
  const { t } = useTranslation();
  const status = useRunStatusFull();
  const hidden = useDocumentHidden();
  const chronological = status?.events ?? [];
  const events = useMemo(() => {
    const list = status?.events ?? [];
    const coalesced = coalesceProgressEvents(list);
    const windowStart = Math.max(0, coalesced.length - 12);
    return coalesced.slice(windowStart).reverse();
  }, [status?.events]);
  const busy = status?.status === 'planning' || status?.status === 'rendering';
  const liveCredits = Math.max(
    0,
    Number(creditsConsumed ?? status?.credits_consumed ?? 0) || 0
  );
  const [nowMs, setNowMs] = useState(() => Date.now());
  useEffect(() => {
    if (!busy || hidden) return;
    setNowMs(Date.now());
    const id = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [busy, hidden]);

  const failure = useMemo(() => {
    if (!status?.error) return null;
    return classifyFailure(status.error, status.stage, status.events, t);
  }, [status?.error, status?.stage, status?.events, t]);

  const staleHint = useMemo(() => {
    if (!status || (status.status !== 'planning' && status.status !== 'rendering')) {
      return null;
    }
    const raw = status.updated_at || status.events?.[status.events.length - 1]?.at;
    if (!raw) return null;
    const ts = Date.parse(raw);
    if (Number.isNaN(ts)) return null;
    const ageSec = (Date.now() - ts) / 1000;
    if (ageSec < 90) return null;
    return t('videoGeneration.workspace.progress.stale');
  }, [status, t]);

  // Latest Seedance poll report: elapsed seconds for the "cloud rendering" line.
  const pollReport = useMemo(() => {
    const evs = status?.events ?? [];
    for (let i = evs.length - 1; i >= 0; i--) {
      if (evs[i].stage !== 'video_poll') continue;
      const meta = evs[i].metadata as { elapsed_secs?: number; status?: number } | null | undefined;
      if (typeof meta?.elapsed_secs === 'number') {
        return meta.elapsed_secs;
      }
    }
    return null;
  }, [status?.events]);

  if (!status) {
    return (
      <div className='text-12px text-[var(--color-text-3)] py-8px'>
        {t('videoGeneration.workspace.progressIdle')}
      </div>
    );
  }

  const progress = Math.max(0, Math.min(100, Number(status.progress) || 0));
  const currentStage = stageLabel(status.stage, t);

  const relatedModel =
    failure?.kind === 'llm'
      ? models?.llm_model
      : failure?.kind === 'image'
        ? models?.image_model
        : failure?.kind === 'video' || failure?.kind === 'moderation'
          ? models?.video_model
          : undefined;

  return (
    <div className='flex flex-col gap-12px'>
      <div className='flex items-center justify-between gap-8px flex-wrap'>
        <div className='flex items-center gap-8px min-w-0'>
          <Tag size='small' color={statusTagColor(status.status)}>
            {statusLabel(status.status, t)}
          </Tag>
          {busy ? <Spin size={14} /> : null}
          {liveCredits > 0 ? (
            <span
              data-testid='session-video-credits-live'
              className='text-12px tabular-nums text-[var(--color-text-3)]'
            >
              {t('videoGeneration.studio.creditsConsumed', {
                credits: liveCredits,
                defaultValue: '消耗 {{credits}} 积分',
              })}
            </span>
          ) : null}
        </div>
        {busy && onCancel ? (
          <Button size='mini' status='danger' loading={cancelling} onClick={onCancel}>
            {t('videoGeneration.workspace.cancel')}
          </Button>
        ) : null}
      </div>

      <div
        className={[
          'rd-8px px-12px py-10px border border-solid',
          busy
            ? 'border-[rgba(var(--primary-6),0.35)] bg-[rgba(var(--primary-6),0.06)]'
            : 'border-[var(--color-border-2)] bg-[var(--color-fill-1)]',
        ].join(' ')}
      >
        <div className='text-11px text-[var(--color-text-3)] mb-2px'>
          {t('videoGeneration.workspace.progress.now')}
        </div>
        <div className='text-14px font-600 text-[var(--color-text-1)] leading-22px'>
          {currentStage ||
            (busy
              ? t('videoGeneration.workspace.progress.working')
              : t('videoGeneration.workspace.progress.idleStep'))}
        </div>
        {status.message &&
        status.message.trim() &&
        status.message.trim() !== currentStage &&
        !/^(planning|rendering|cancelled|interrupted)$/i.test(status.message.trim()) ? (
          <div className='text-12px leading-18px text-[var(--color-text-2)] mt-2px'>
            {status.message.trim()}
          </div>
        ) : null}
        {status.stage === 'video_poll' && typeof pollReport === 'number' ? (
          <div className='text-12px leading-18px text-[var(--color-text-2)] mt-2px tabular-nums'>
            {t('videoGeneration.workspace.progress.pollWait', {
              secs: pollReport,
              defaultValue: '已等待 {{secs}} 秒',
            })}
          </div>
        ) : null}
        {staleHint ? (
          <div className='text-12px leading-18px text-[rgb(var(--warning-6))] mt-6px'>
            {staleHint}
          </div>
        ) : null}
      </div>

      {status.working_dir_abs ? (
        <div className='text-11px text-[var(--color-text-3)] break-all'>
          {t('videoGeneration.workspace.progress.workdirLine', {
            path: status.working_dir_abs,
            defaultValue: '工作目录：{{path}}',
          })}
        </div>
      ) : null}

      {busy || progress > 0 ? (
        <Progress
          percent={busy && progress < 3 ? 3 : progress}
          animation={busy}
          showText
          size='small'
        />
      ) : null}

      {status.error && failure ? (
        <div className='rd-8px px-12px py-10px border border-solid border-[rgba(var(--danger-6),0.35)] bg-[rgba(var(--danger-6),0.06)] flex flex-col gap-6px'>
          <div className='text-13px font-600 text-[rgb(var(--danger-6))]'>{failure.title}</div>
          <div className='text-12px leading-18px text-[var(--color-text-1)]'>{failure.hint}</div>
          {relatedModel ? (
            <div className='text-11px text-[var(--color-text-3)]'>
              {t('videoGeneration.workspace.progress.currentModel', { model: relatedModel })}
            </div>
          ) : null}
          <details open className='text-11px text-[var(--color-text-3)]'>
            <summary className='cursor-pointer select-none'>
              {t('videoGeneration.workspace.progress.errorDetail')}
            </summary>
            <pre className='m-0 mt-6px whitespace-pre-wrap break-all font-mono leading-16px text-[rgb(var(--danger-6))]'>
              {status.error}
            </pre>
          </details>
        </div>
      ) : null}

      {events.length > 0 ? (
        <div className='flex flex-col gap-4px'>
          <div className='text-11px text-[var(--color-text-3)]'>
            {t('videoGeneration.workspace.progress.log')}
          </div>
          <div className='max-h-160px overflow-y-auto rd-8px border border-solid border-[var(--color-border-2)] bg-[var(--color-fill-1)] px-10px py-8px flex flex-col gap-6px'>
            {events.map(({ event: ev, index, count }) => {
              const label = stageLabel(ev.stage, t);
              const elapsed = eventElapsed(chronological, index, {
                busy,
                nowMs,
                updatedAt: status.updated_at,
                untilIndex: index + count,
              });
              const clock =
                elapsed.secs == null ? '' : formatElapsedClock(elapsed.secs);
              // Never show backend Chinese messages in the activity log — stage label is enough.
              return (
                <div
                  key={`${ev.at}-${ev.stage}-${index}`}
                  className='flex gap-8px text-11px leading-16px items-baseline'
                >
                  <span className='shrink-0 text-[var(--color-text-3)] tabular-nums'>
                    {formatEventTime(ev.at)}
                  </span>
                  <span
                    className={[
                      'min-w-0 flex-1 font-500',
                      ev.stage === 'failed'
                        ? 'text-[rgb(var(--danger-6))]'
                        : 'text-[rgb(var(--primary-6))]',
                    ].join(' ')}
                  >
                    {label}
                  </span>
                  {clock ? (
                    <span
                      className={[
                        'shrink-0 tabular-nums',
                        elapsed.live
                          ? 'text-[rgb(var(--primary-6))]'
                          : 'text-[var(--color-text-3)]',
                      ].join(' ')}
                      aria-live={elapsed.live ? 'polite' : undefined}
                    >
                      {elapsed.live
                        ? t('videoGeneration.workspace.progress.elapsedLive', {
                            time: clock,
                            defaultValue: '{{time}}',
                          })
                        : t('videoGeneration.workspace.progress.elapsedDone', {
                            time: clock,
                            defaultValue: '耗时 {{time}}',
                          })}
                    </span>
                  ) : null}
                </div>
              );
            })}
          </div>
        </div>
      ) : null}
    </div>
  );
};

function formatEventTime(at: string | undefined): string {
  if (!at) return '';
  const d = new Date(at);
  if (Number.isNaN(d.getTime())) return at.slice(11, 19) || at;
  return d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

export default ProgressTimeline;
