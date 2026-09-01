import React, { useMemo, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { CREDITS_PER_SECOND, formatDurationCredits } from '../durationCredits';
import {
  AGENT_TICKS,
  CLIP_TICKS,
  clampDuration,
  DURATION_MAX_SECS,
  DURATION_MIN_SECS,
  DURATION_STEP_SECS,
} from '../durationBounds';
import styles from './durationTimeline.module.css';

export {
  AGENT_TICKS,
  CLIP_DURATION_DEFAULT_SECS,
  CLIP_DURATION_MAX_SECS,
  CLIP_DURATION_MIN_SECS,
  CLIP_DURATION_STEP_SECS,
  CLIP_TICKS,
  clampDuration,
  DURATION_MAX_SECS,
  DURATION_MIN_SECS,
  DURATION_STEP_SECS,
} from '../durationBounds';

interface DurationTimelineBarProps {
  value: number;
  onChange?: (secs: number) => void;
  disabled?: boolean;
  /** Stretch across a wider grid column on the home composer. */
  wide?: boolean;
  /** Hide the built-in title when an outer section label already exists. */
  hideLabel?: boolean;
  min?: number;
  max?: number;
  step?: number;
  ticks?: readonly number[];
  /** Hide the video-credit estimate (briefing length is not billed per second). */
  hideCredits?: boolean;
}

const DurationTimelineBar: React.FC<DurationTimelineBarProps> = ({
  value,
  onChange,
  disabled,
  wide,
  hideLabel,
  min = DURATION_MIN_SECS,
  max = DURATION_MAX_SECS,
  step = DURATION_STEP_SECS,
  ticks,
  hideCredits,
}) => {
  const { t } = useTranslation();
  const scrubRef = useRef<HTMLDivElement | null>(null);
  const draggingRef = useRef(false);
  const resolvedTicks = ticks ?? (max <= 30 ? CLIP_TICKS : AGENT_TICKS);
  const visibleTicks = useMemo(
    () => resolvedTicks.filter((tick) => tick >= min && tick <= max),
    [resolvedTicks, min, max]
  );
  const secs = clampDuration(value, min, max, step);
  const progress = useMemo(() => {
    const span = Math.max(1, max - min);
    return ((secs - min) / span) * 100;
  }, [secs, min, max]);

  const seekFromClientX = (clientX: number) => {
    if (disabled || !onChange) return;
    const el = scrubRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    if (rect.width <= 0) return;
    const ratio = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
    const raw = min + ratio * (max - min);
    onChange(clampDuration(raw, min, max, step));
  };

  const onScrubPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (disabled || !onChange) return;
    event.preventDefault();
    draggingRef.current = true;
    event.currentTarget.setPointerCapture(event.pointerId);
    seekFromClientX(event.clientX);
  };

  const onScrubPointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    if (!draggingRef.current) return;
    seekFromClientX(event.clientX);
  };

  const onScrubPointerUp = (event: React.PointerEvent<HTMLDivElement>) => {
    if (!draggingRef.current) return;
    draggingRef.current = false;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  return (
    <div
      className={[
        styles.durationTimeline,
        wide ? styles.durationTimelineWide : '',
        hideLabel ? styles.durationTimelineCompact : '',
        disabled ? styles.durationTimelineDisabled : '',
      ]
        .filter(Boolean)
        .join(' ')}
    >
      <div className={styles.durationTimelineHeader}>
        {hideLabel ? (
          <span className={styles.durationTimelineHint}>
            {t('videoGeneration.workspace.source.durationHintShort', {
              defaultValue: '拖动或点击刻度选择',
            })}
          </span>
        ) : (
          <span className={styles.durationTimelineLabel}>
            {t('videoGeneration.workspace.source.durationLabel', {
              defaultValue: '目标时长（秒）',
            })}
          </span>
        )}
        <span className={styles.durationTimelineValue} aria-live='polite'>
          <span className={styles.durationTimelineValueNum}>{secs}</span>
          <span className={styles.durationTimelineUnit}>s</span>
        </span>
      </div>

      <div className={styles.durationTimelineBody}>
        <div
          ref={scrubRef}
          className={styles.durationTimelineScrub}
          onPointerDown={onScrubPointerDown}
          onPointerMove={onScrubPointerMove}
          onPointerUp={onScrubPointerUp}
          onPointerCancel={onScrubPointerUp}
        >
          <div className={styles.durationTimelineRail} aria-hidden>
            <div className={styles.durationTimelineFill} style={{ width: `${progress}%` }} />
          </div>
          <div
            className={styles.durationTimelineThumb}
            style={{ left: `${progress}%` }}
            aria-hidden
          />
          <input
            className={styles.durationTimelineRange}
            type='range'
            min={min}
            max={max}
            step={step}
            value={secs}
            disabled={disabled}
            tabIndex={disabled ? -1 : 0}
            aria-valuemin={min}
            aria-valuemax={max}
            aria-valuenow={secs}
            aria-label={t('videoGeneration.workspace.source.durationLabel', {
              defaultValue: '目标时长（秒）',
            })}
            onChange={(event) =>
              onChange?.(clampDuration(Number(event.target.value), min, max, step))
            }
          />
        </div>

        <div className={styles.durationTimelineTicks}>
          {visibleTicks.map((tick) => {
            const left = ((tick - min) / Math.max(1, max - min)) * 100;
            const active = secs >= tick;
            const current = secs === tick;
            return (
              <button
                key={tick}
                type='button'
                className={[
                  styles.durationTimelineTick,
                  active ? styles.durationTimelineTickActive : '',
                  current ? styles.durationTimelineTickCurrent : '',
                ]
                  .filter(Boolean)
                  .join(' ')}
                style={{ left: `${left}%` }}
                disabled={disabled}
                onClick={() => onChange?.(clampDuration(tick, min, max, step))}
                aria-label={`${tick}s`}
                aria-pressed={current}
              >
                <span className={styles.durationTimelineTickMark} />
                <span className={styles.durationTimelineTickLabel}>{tick}</span>
              </button>
            );
          })}
        </div>
      </div>

      <div className={styles.durationTimelineFooter}>
        {hideCredits ? (
          <span className={styles.durationTimelineCredits}>
            {t('videoGeneration.briefing.formatRange', {
              min,
              max,
              defaultValue: '{{min}}–{{max}} 秒口播',
            })}
          </span>
        ) : (
          <span className={styles.durationTimelineCredits}>
            {t('videoGeneration.workspace.source.durationCreditsHint', {
              credits: formatDurationCredits(secs),
              rate: CREDITS_PER_SECOND,
              defaultValue: '预估约 {{credits}} 积分（约 {{rate}}/秒）',
            })}
          </span>
        )}
        <span className={styles.durationTimelineRangeHint}>
          {min}–{max}s
        </span>
      </div>
    </div>
  );
};

export default DurationTimelineBar;
