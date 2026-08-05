import React, { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { CREDITS_PER_SECOND, formatDurationCredits } from '../durationCredits';
import styles from '../index.module.css';

export const DURATION_MIN_SECS = 5;
export const DURATION_MAX_SECS = 300;
export const DURATION_STEP_SECS = 5;

const TICKS = [15, 30, 60, 90, 120, 180, 240, 300] as const;

interface DurationTimelineBarProps {
  value: number;
  onChange?: (secs: number) => void;
  disabled?: boolean;
  /** Stretch across a wider grid column on the home composer. */
  wide?: boolean;
}

function clampDuration(secs: number): number {
  if (!Number.isFinite(secs)) return 30;
  const stepped = Math.round(secs / DURATION_STEP_SECS) * DURATION_STEP_SECS;
  return Math.min(DURATION_MAX_SECS, Math.max(DURATION_MIN_SECS, stepped));
}

const DurationTimelineBar: React.FC<DurationTimelineBarProps> = ({
  value,
  onChange,
  disabled,
  wide,
}) => {
  const { t } = useTranslation();
  const secs = clampDuration(value);
  const progress = useMemo(() => {
    const span = DURATION_MAX_SECS - DURATION_MIN_SECS;
    return ((secs - DURATION_MIN_SECS) / span) * 100;
  }, [secs]);

  return (
    <div
      className={[styles.durationTimeline, wide ? styles.durationTimelineWide : '', disabled ? styles.durationTimelineDisabled : '']
        .filter(Boolean)
        .join(' ')}
    >
      <div className={styles.durationTimelineHeader}>
        <span className={styles.durationTimelineLabel}>
          {t('videoGeneration.workspace.source.durationLabel', {
            defaultValue: '目标时长（秒）',
          })}
        </span>
        <span className={styles.durationTimelineValue} aria-live='polite'>
          {secs}
          <span className={styles.durationTimelineUnit}>s</span>
        </span>
      </div>

      <div className={styles.durationTimelineTrackWrap}>
        <div className={styles.durationTimelineTrack} aria-hidden>
          <div className={styles.durationTimelineFill} style={{ width: `${progress}%` }} />
          <div className={styles.durationTimelineTicks}>
            {TICKS.map((tick) => {
              const left =
                ((tick - DURATION_MIN_SECS) / (DURATION_MAX_SECS - DURATION_MIN_SECS)) * 100;
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
                  onClick={() => onChange?.(tick)}
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
          min={DURATION_MIN_SECS}
          max={DURATION_MAX_SECS}
          step={DURATION_STEP_SECS}
          value={secs}
          disabled={disabled}
          aria-valuemin={DURATION_MIN_SECS}
          aria-valuemax={DURATION_MAX_SECS}
          aria-valuenow={secs}
          aria-label={t('videoGeneration.workspace.source.durationLabel', {
            defaultValue: '目标时长（秒）',
          })}
          onChange={(event) => onChange?.(clampDuration(Number(event.target.value)))}
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
          {DURATION_MIN_SECS}–{DURATION_MAX_SECS}s
        </span>
      </div>
    </div>
  );
};

export default DurationTimelineBar;
