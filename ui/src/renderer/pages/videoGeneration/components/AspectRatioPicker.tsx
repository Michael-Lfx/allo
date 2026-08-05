/**
 * Seedance aspect ratio picker — pill buttons, not Select-in-<label>
 * (native labels swallow / steal Arco Select clicks intermittently).
 */
import React, { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import {
  DEFAULT_SEEDANCE_ASPECT_RATIO,
  SEEDANCE_ASPECT_RATIOS,
  normalizeSeedanceAspectRatio,
  type SeedanceAspectRatio,
} from '../aspectRatios';
import styles from '../index.module.css';

export interface AspectRatioPickerProps {
  value: string;
  onChange: (next: SeedanceAspectRatio) => void;
  disabled?: boolean;
  className?: string;
  /** Hide the live frame preview under the pills. */
  hidePreview?: boolean;
}

function parseRatio(ratio: SeedanceAspectRatio): { w: number; h: number } {
  const [a, b] = ratio.split(':').map((part) => Number(part));
  if (!Number.isFinite(a) || !Number.isFinite(b) || a <= 0 || b <= 0) {
    return { w: 16, h: 9 };
  }
  return { w: a, h: b };
}

const AspectRatioPicker: React.FC<AspectRatioPickerProps> = ({
  value,
  onChange,
  disabled,
  className,
  hidePreview,
}) => {
  const { t } = useTranslation();
  const current = normalizeSeedanceAspectRatio(value || DEFAULT_SEEDANCE_ASPECT_RATIO);
  const { w, h } = useMemo(() => parseRatio(current), [current]);

  return (
    <div className={`flex flex-col gap-8px ${className ?? ''}`.trim()}>
      <div className='flex flex-wrap gap-6px' role='radiogroup' aria-label='aspect ratio'>
        {SEEDANCE_ASPECT_RATIOS.map((ratio) => {
          const active = ratio === current;
          return (
            <button
              key={ratio}
              type='button'
              role='radio'
              aria-checked={active}
              disabled={disabled}
              onClick={(event) => {
                event.preventDefault();
                event.stopPropagation();
                if (disabled || ratio === current) return;
                onChange(ratio);
              }}
              className={[
                'm-0 cursor-pointer border border-solid px-10px py-5px text-12px leading-18px transition-colors',
                'rd-8px disabled:cursor-not-allowed disabled:opacity-50',
                active
                  ? 'border-[rgb(var(--primary-6))] bg-[rgb(var(--primary-1))] text-[rgb(var(--primary-6))] font-600'
                  : 'border-[var(--color-border-2)] bg-[var(--color-bg-2)] text-[var(--color-text-2)] hover:border-[rgb(var(--primary-5))]',
              ].join(' ')}
            >
              {ratio}
            </button>
          );
        })}
      </div>

      {hidePreview ? null : (
        <div
          className={styles.aspectPreview}
          aria-live='polite'
          aria-label={t('videoGeneration.workspace.source.aspectPreview', {
            ratio: current,
            defaultValue: '画幅预览 {{ratio}}',
          })}
        >
          <div className={styles.aspectPreviewStage}>
            <div
              className={styles.aspectPreviewFrame}
              style={{ aspectRatio: `${w} / ${h}` }}
            >
              <span className={styles.aspectPreviewLabel}>{current}</span>
            </div>
          </div>
          <div className={styles.aspectPreviewHint}>
            {t('videoGeneration.workspace.source.aspectPreviewHint', {
              ratio: current,
              defaultValue: '成片与海报将按 {{ratio}} 构图',
            })}
          </div>
        </div>
      )}
    </div>
  );
};

export default AspectRatioPicker;
