import React, { useId } from 'react';

type FlowyLogoProps = {
  className?: string;
  size?: number;
  title?: string;
};

const OUTER_PATH =
  'M1027 11984 c-388 -60 -703 -282 -885 -624 -49 -90 -95 -220 -119 -332 -17 -79 -18 -320 -18 -5023 0 -4669 1 -4945 18 -5026 34 -169 91 -308 181 -443 173 -260 429 -433 758 -513 69 -17 308 -18 4993 -20 3145 -1 4945 2 4990 8 479 67 860 386 999 839 58 186 54 -153 52 5185 -2 4599 -3 4925 -19 4995 -82 362 -283 639 -592 814 -88 51 -210 98 -335 129 l-85 21 -4925 2 c-4139 2 -4939 0 -5013 -12z';

const LETTER_PATH =
  'M8800 9200 l0 -800 -2000 0 -2000 0 0 -800 0 -800 -800 0 -800 0 0 1600 0 1600 2800 0 2800 0 0 -800 z M8000 6000 l0 -800 -800 0 -800 0 0 800 0 800 800 0 800 0 0 -800 z M4800 3600 l0 -1600 -800 0 -800 0 0 1600 0 1600 800 0 800 0 0 -1600 z';

const ICON_TRANSFORM = 'translate(0,1200) scale(0.1,-0.1)';

const FlowyLogo: React.FC<FlowyLogoProps> = ({ className, size = 64, title = 'Flowy' }) => {
  const clipId = useId();

  return (
    <svg
      xmlns='http://www.w3.org/2000/svg'
      viewBox='14 2 1184 1184'
      width={size}
      height={size}
      fill='none'
      role='img'
      aria-label={title}
      className={className}
    >
      <title>{title}</title>
      <defs>
        <clipPath id={clipId}>
          <path transform={ICON_TRANSFORM} d={OUTER_PATH} />
        </clipPath>
      </defs>
      <g clipPath={`url(#${clipId})`}>
        <g transform={ICON_TRANSFORM}>
          <path fill='#000' d={OUTER_PATH} />
          <path fill='#fff' d={LETTER_PATH} />
        </g>
      </g>
    </svg>
  );
};

export default FlowyLogo;
