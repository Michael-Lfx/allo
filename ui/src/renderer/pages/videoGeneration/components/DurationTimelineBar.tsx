import React, { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { CREDITS_PER_SECOND, formatDurationCredits } from '../durationCredits';
import styles from '../index.module.css';

export const DURATION_MIN_SECS = 5;
export const DURATION_MAX_SECS = 300;
export const DURATION_STEP_SECS = 5;

/** Single-clip duration for Canvas / creation mode (Seedance-style). */
export const CLIP_DURATION_MIN_SECS = 4;
export const CLIP_DURATION_MAX_SECS = 15;
export const CLIP_DURATION_STEP_SECS = 1;
export const CLIP_DURATION_DEFAULT_SECS = 6;

const AGENT_TICKS = [15, 30, 60, 90, 120, 180, 240, 300] as const;
export const CLIP_TICKS = [4, 6, 8, 10, 12, 15] as const;

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
}

export function clampDuration(
  secs: number,
  min = DURATION_MIN_SECS,
  max = DURATION_MAX_SECS,
  step = DURATION_STEP_SECS
): number {
  if (!Number.isFinite(secs)) {
    return Math.min(max, Math.max(min, min <= 30 && max >= 30 ? 30 : min));
  }
  const stepped = Math.round(secs / step) * step;
  return Math.min(max, Math.max(min, stepped));
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
}) => {
  const { t } = useTranslation();
  const resolvedTicks = ticks ?? (max <= 30 ? CLIP_TICKS : AGENT_TICKS);
  const secs = clampDuration(value, min, max, step);
  const progress = useMemo(() => {
    const span = Math.max(1, max - min);
    return ((secs - min) / span) * 100;
  }, [secs, min, max]);

  return (
    <div
      className={[
        styles.durationTimeline,
        wide ? styles.durationTimelineWide : '',
        disabled ? styles.durationTimelineDisabled : '',
      ]
        .filter(Boolean)
        .join(' ')}
    >
      <div className={styles.durationTimelineHeader}>
        {hideLabel ? (
          <span />
        ) : (
          <span className={styles.durationTimelineLabel}>
            {t('videoGeneration.workspace.source.durationLabel', {
              defaultValue: '目标时长（秒）',
            })}
          </span>
        )}
        <span className={styles.durationTimelineValue} aria-live='polite'>
          {secs}
          <span className={styles.durationTimelineUnit}>s</span>
        </span>
      </div>

      <div className={styles.durationTimelineTrackWrap}>
        <div className={styles.durationTimelineTrack} aria-hidden>
          <div className={styles.durationTimelineFill} style={{ width: `${progress}%` }} />
          <div className={styles.durationTimelineTicks}>
            {resolvedTicks
              .filter((tick) => tick >= min && tick <= max)
              .map((tick) => {
                const left = ((tick - min) / Math.max(1, max - min)) * 100;
                return (
                  <button
                    key={tick}
                    type='button'
                    className={[
                      styles.durationTimelineTick,
                      secs >= tick ? styles.durationTimelineTickActive : '',
                      secs === tick ? styles.durationTimelineTickCurrent : '',
                    ]
                      .filter(Boolean)
                      .join(' ')}
                    style={{ left: `${left}%` }}
                    disabled={disabled}
                    onClick={() => onChange?.(clampDuration(tick, min, max, step))}
                    aria-label={`${tick}s`}
                  >
                    <span className={styles.durationTimelineTickMark} />
                    <span className={styles.durationTimelineTickLabel}>{tick}</span>
                  </button>
                );
              })}
          </div>
        </div>
        <input
          className={styles.durationTimelineRange}
          type='range'
          min={min}
          max={max}
          step={step}
          value={secs}
          disabled={disabled}
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

      <div className={styles.durationTimelineFooter}>
        <span className={styles.durationTimelineCredits}>
          {t('videoGeneration.workspace.source.durationCreditsHint', {
            credits: formatDurationCredits(secs),
            rate: CREDITS_PER_SECOND,
            defaultValue: '预估约 {{credits}} 积分（约 {{rate}}/秒）',
          })}
        </span>
        <span className={styles.durationTimelineRangeHint}>
          {min}–{max}s
        </span>
      </div>
    </div>
  );
};

export default DurationTimelineBar;
