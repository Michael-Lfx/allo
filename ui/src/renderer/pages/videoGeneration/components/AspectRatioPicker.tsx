/**
 * Seedance aspect ratio picker — pill buttons, not Select-in-<label>
 * (native labels swallow / steal Arco Select clicks intermittently).
 */
import React from 'react';
import {
  DEFAULT_SEEDANCE_ASPECT_RATIO,
  SEEDANCE_ASPECT_RATIOS,
  normalizeSeedanceAspectRatio,
  type SeedanceAspectRatio,
} from '../aspectRatios';

export interface AspectRatioPickerProps {
  value: string;
  onChange: (next: SeedanceAspectRatio) => void;
  disabled?: boolean;
  className?: string;
}

const AspectRatioPicker: React.FC<AspectRatioPickerProps> = ({
  value,
  onChange,
  disabled,
  className,
}) => {
  const current = normalizeSeedanceAspectRatio(value || DEFAULT_SEEDANCE_ASPECT_RATIO);

  return (
    <div
      className={`flex flex-wrap gap-6px ${className ?? ''}`.trim()}
      role='radiogroup'
      aria-label='aspect ratio'
    >
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
  );
};

export default AspectRatioPicker;
