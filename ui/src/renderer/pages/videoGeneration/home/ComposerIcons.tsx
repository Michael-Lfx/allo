import React from 'react';

/** Slanted document / attachment glyph for the home composer upload slot. */
export function SlantedDocIcon({
  size = 22,
  className,
}: {
  size?: number;
  className?: string;
}): React.ReactElement {
  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox='0 0 24 24'
      fill='none'
      xmlns='http://www.w3.org/2000/svg'
      aria-hidden='true'
    >
      <g transform='rotate(-22 12 12)'>
        <rect
          x='6.25'
          y='3.5'
          width='11.5'
          height='17'
          rx='2.25'
          stroke='currentColor'
          strokeWidth='2'
        />
        <path
          d='M9 9.25h6M9 12.5h6M9 15.75h4'
          stroke='currentColor'
          strokeWidth='1.85'
          strokeLinecap='round'
        />
      </g>
    </svg>
  );
}

/** Circular plus used by the Jimeng-style attach trigger. */
export function AttachPlusIcon({
  size = 16,
  className,
}: {
  size?: number;
  className?: string;
}): React.ReactElement {
  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox='0 0 16 16'
      fill='none'
      xmlns='http://www.w3.org/2000/svg'
      aria-hidden='true'
    >
      <path
        d='M8 3.1v9.8M3.1 8h9.8'
        stroke='currentColor'
        strokeWidth='1.8'
        strokeLinecap='round'
      />
    </svg>
  );
}

/** White close mark for the Jimeng-style thumbnail corner chip. */
export function AttachCloseIcon(): React.ReactElement {
  return (
    <svg width='8' height='8' viewBox='0 0 8 8' fill='none' aria-hidden='true'>
      <path
        d='M1.15 1.15 6.85 6.85M6.85 1.15 1.15 6.85'
        stroke='currentColor'
        strokeWidth='1.45'
        strokeLinecap='round'
      />
    </svg>
  );
}

/** Bold circular send arrow for the home composer submit control. */
export function BoldSendArrowIcon({
  size = 18,
  className,
}: {
  size?: number;
  className?: string;
}): React.ReactElement {
  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox='0 0 24 24'
      fill='none'
      xmlns='http://www.w3.org/2000/svg'
      aria-hidden='true'
    >
      <path
        d='M12 19.25V6.4'
        stroke='currentColor'
        strokeWidth='2.85'
        strokeLinecap='round'
      />
      <path
        d='M6.2 11.35 12 5.55l5.8 5.8'
        stroke='currentColor'
        strokeWidth='2.85'
        strokeLinecap='round'
        strokeLinejoin='round'
      />
    </svg>
  );
}
