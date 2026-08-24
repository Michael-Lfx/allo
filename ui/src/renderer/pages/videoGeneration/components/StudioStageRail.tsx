import React, { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Spin } from '@arco-design/web-react';
import type { VimaxRunStatus } from '../types';
import { formatElapsedClock } from '../progressEventElapsed';
import {
  buildStudioStageTimeline,
  type StudioStageKey,
  type StudioStageSegment,
  type StudioStageState,
  type StudioStageVariant,
} from '../studioStageTimeline';
import { useDocumentHidden, useRunStatusFull } from '../useRunStatusFeed';
import styles from '../index.module.css';

interface StudioStageRailProps {
  hasStoryboard: boolean;
  hasFinalVideo: boolean;
  /** `action` = action-imitation runs, which use the 3-phase rail. */
  variant?: StudioStageVariant;
}

const STAGE_LABEL_KEYS: Record<StudioStageKey, { key: string; fallback: string }> = {
  brief: { key: 'videoGeneration.studio.stages.brief', fallback: '创意' },
  storyboard: { key: 'videoGeneration.studio.stages.storyboard', fallback: '分镜' },
  render: { key: 'videoGeneration.studio.stages.render', fallback: '渲染' },
  film: { key: 'videoGeneration.studio.stages.film', fallback: '成片' },
  assets: { key: 'videoGeneration.studio.stages.assets', fallback: '素材' },
  generate: { key: 'videoGeneration.studio.stages.generate', fallback: '生成' },
};

const STATE_LABEL_KEYS: Record<StudioStageState, { key: string; fallback: string }> = {
  pending: { key: 'videoGeneration.studio.stageState.pending', fallback: '待开始' },
  active: { key: 'videoGeneration.studio.stageState.active', fallback: '进行中' },
  done: { key: 'videoGeneration.studio.stageState.done', fallback: '已完成' },
  failed: { key: 'videoGeneration.studio.stageState.failed', fallback: '失败' },
  cancelled: { key: 'videoGeneration.studio.stageState.cancelled', fallback: '已取消' },
};

const STATE_CLASS: Record<StudioStageState, string> = {
  pending: '',
  active: styles.stageSegmentActive,
  done: styles.stageSegmentDone,
  failed: styles.stageSegmentFailed,
  cancelled: styles.stageSegmentCancelled,
};

function formatClockTime(ms: number | null): string {
  if (ms == null) return '';
  const date = new Date(ms);
  if (Number.isNaN(date.getTime())) return '';
  return date.toLocaleTimeString(undefined, {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

function StageGlyph({ kind }: { kind: 'check' | 'close' }) {
  return (
    <svg
      width='12'
      height='12'
      viewBox='0 0 16 16'
      fill='none'
      aria-hidden='true'
      focusable='false'
    >
      {kind === 'check' ? (
        <path
          d='M3.15 8.35 6.4 11.35 12.45 4.9'
          stroke='currentColor'
          strokeWidth='2.2'
          strokeLinecap='round'
          strokeLinejoin='round'
        />
      ) : (
        <path
          d='M4.5 4.5 11.5 11.5 M11.5 4.5 4.5 11.5'
          stroke='currentColor'
          strokeWidth='2.2'
          strokeLinecap='round'
        />
      )}
    </svg>
  );
}

const StageMarker: React.FC<{ state: StudioStageState; index: number }> = ({ state, index }) => {
  switch (state) {
    case 'done':
      return <StageGlyph kind='check' />;
    case 'active':
      return <Spin size={12} />;
    case 'failed':
    case 'cancelled':
      return <StageGlyph kind='close' />;
    case 'pending':
      return <>{index + 1}</>;
    default: {
      const exhaustive: never = state;
      return exhaustive;
    }
  }
};

const StudioStageRail: React.FC<StudioStageRailProps> = ({
  hasStoryboard,
  hasFinalVideo,
  variant = 'film',
}) => {
  const { t } = useTranslation();
  const runStatus = useRunStatusFull();
  const hidden = useDocumentHidden();
  const status: VimaxRunStatus | null = runStatus?.status ?? null;
  const busy = status === 'planning' || status === 'rendering';
  const [nowMs, setNowMs] = useState(() => Date.now());

  useEffect(() => {
    if (!busy || hidden) return;
    setNowMs(Date.now());
    const id = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [busy, hidden]);

  const segments = useMemo(
    () =>
      buildStudioStageTimeline({
        status,
        stage: runStatus?.stage ?? null,
        events: runStatus?.events,
        updatedAt: runStatus?.updated_at ?? null,
        nowMs,
        hasStoryboard,
        hasFinalVideo,
        variant,
      }),
    [status, runStatus?.stage, runStatus?.events, runStatus?.updated_at, nowMs, hasStoryboard, hasFinalVideo, variant]
  );

  return (
    <nav
      className={styles.stageRail}
      aria-label={t('videoGeneration.studio.stageLabel', { defaultValue: '影片制作进度' })}
    >
      {segments.map((segment: StudioStageSegment, index) => {
        const label = t(STAGE_LABEL_KEYS[segment.key].key, {
          defaultValue: STAGE_LABEL_KEYS[segment.key].fallback,
        });
        const stateText = t(STATE_LABEL_KEYS[segment.state].key, {
          defaultValue: STATE_LABEL_KEYS[segment.state].fallback,
        });
        const clock =
          segment.durationMs == null
            ? ''
            : formatElapsedClock(Math.round(segment.durationMs / 1000));
        const startedAt = formatClockTime(segment.startedAtMs);

        return (
          <div
            key={segment.key}
            className={[styles.stageSegment, STATE_CLASS[segment.state]]
              .filter(Boolean)
              .join(' ')}
            style={{ flexGrow: segment.weight }}
            aria-current={segment.state === 'active' ? 'step' : undefined}
            aria-label={[label, stateText, clock].filter(Boolean).join(' · ')}
            title={
              startedAt
                ? t('videoGeneration.studio.stageStartedAt', {
                    time: startedAt,
                    defaultValue: '开始于 {{time}}',
                  })
                : undefined
            }
          >
            <div className={styles.stageSegmentHead}>
              <span className={styles.stageDot}>
                <StageMarker state={segment.state} index={index} />
              </span>
              <span className={styles.stageLabel}>{label}</span>
              {clock ? (
                <span
                  className={styles.stageClock}
                  aria-live={segment.live ? 'polite' : undefined}
                >
                  {clock}
                </span>
              ) : null}
            </div>
            <div className={styles.stageSegmentBar} />
            <div className={styles.stageSegmentFoot}>{startedAt}</div>
          </div>
        );
      })}
    </nav>
  );
};

export default StudioStageRail;
