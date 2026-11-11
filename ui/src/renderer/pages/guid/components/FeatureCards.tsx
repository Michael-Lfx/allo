import React from 'react';
import { useTranslation } from 'react-i18next';
import styles from './FeatureCards.module.css';

/** Rounded window outline; a path keeps pathLength/dash tracing consistent. */
const WINDOW_FRAME_PATH = 'M35 10h106a9 9 0 0 1 9 9v58a9 9 0 0 1-9 9H35a9 9 0 0 1-9-9V19a9 9 0 0 1 9-9Z';

export interface FeatureCardsProps {
  /** Opens the develop directory (new project / existing project). */
  onOpenDevelop: () => void;
  /** Opens the create directory (slides / video). */
  onOpenCreate: () => void;
}

/**
 * Home stage entries: two stacked capability buttons (develop / create).
 * Both illustrations use one fixed 176x96 window; hover crossfades the inner
 * content layer from code/chart to a rendered page/bar chart. The window frame,
 * copy and neighbours stay put. Reduced motion keeps the first state.
 */
const FeatureCards: React.FC<FeatureCardsProps> = ({ onOpenDevelop, onOpenCreate }) => {
  const { t } = useTranslation();

  return (
    <div className={styles.board}>
      <button
        type='button'
        data-testid='guid-home-develop'
        className={`${styles.entry} ${styles.entryButton}`}
        onClick={onOpenDevelop}
      >
        <span className={styles.entryCopy}>
          <span className={styles.entryTitle}>
            {t('guid.home.buildTitle')}
            <Chevron />
          </span>
          <span className={styles.entryDescription}>{t('guid.home.buildDescription')}</span>
        </span>
        <BuildEntryArt />
      </button>

      <button
        type='button'
        data-testid='guid-home-create'
        className={`${styles.entry} ${styles.entryButton}`}
        onClick={onOpenCreate}
      >
        <span className={styles.entryCopy}>
          <span className={styles.entryTitle}>
            {t('guid.home.createTitle')}
            <Chevron />
          </span>
          <span className={styles.entryDescription}>{t('guid.home.createDescription')}</span>
        </span>
        <CreateEntryArt />
      </button>
    </div>
  );
};

function Chevron() {
  return (
    <svg className={styles.chevron} viewBox='0 0 16 16' aria-hidden='true'>
      <path
        d='M6 3.5 10.5 8 6 12.5'
        fill='none'
        stroke='currentColor'
        strokeWidth='1.6'
        strokeLinecap='round'
        strokeLinejoin='round'
      />
    </svg>
  );
}

/** Build: one window whose code surface crossfades into a rendered page. */
function BuildEntryArt() {
  return (
    <svg
      className={styles.entryArt}
      data-art='build'
      viewBox='0 0 176 96'
      aria-hidden='true'
      focusable='false'
      fill='none'
      stroke='currentColor'
      strokeWidth='2'
      strokeLinecap='round'
      strokeLinejoin='round'
    >
      <g className={styles.artWindowFrame}>
        <rect x='26' y='10' width='124' height='76' rx='9' />
        <path d='M26 29h124M38 19h2m6 0h2m6 0h2' />
      </g>
      <g className={styles.artStateCode}>
        <path className={styles.artAccent} d='m110 40-5 5 5 5m12-10 5 5-5 5' />
        <path d='M42 40h28M42 49h44M42 58h26M42 67h38M42 76h22' />
      </g>
      <g className={styles.artStatePage}>
        <rect className={styles.artAccent} x='42' y='38' width='52' height='20' rx='4' />
        <path d='M42 68h20m8 0h24M42 76h14' />
        <rect x='104' y='38' width='30' height='32' rx='4' />
        <path d='M110 47h18M110 55h12M110 63h16' />
      </g>
      <path className={styles.artFrameTrace} d={WINDOW_FRAME_PATH} pathLength={1} />
    </svg>
  );
}

/** Bar heights used by the create card's data-visualization wave. */
const CREATE_BARS = [
  { x: 42, height: 22 },
  { x: 53, height: 38 },
  { x: 64, height: 30 },
  { x: 75, height: 42 },
  { x: 86, height: 26 },
  { x: 97, height: 36 },
  { x: 108, height: 28 },
  { x: 119, height: 40 },
] as const;

/** Create: one media window whose line chart becomes a wave of bars. */
function CreateEntryArt() {
  return (
    <svg
      className={styles.entryArt}
      data-art='create'
      viewBox='0 0 176 96'
      aria-hidden='true'
      focusable='false'
      fill='none'
      stroke='currentColor'
      strokeWidth='2'
      strokeLinecap='round'
      strokeLinejoin='round'
    >
      <g className={styles.artWindowFrame}>
        <rect x='26' y='10' width='124' height='76' rx='9' />
        <path d='M26 29h124M38 19h2m6 0h2m6 0h2' />
      </g>
      <g className={styles.artStateLine}>
        <path className={styles.artAccent} d='M42 38h46' />
        <path d='M42 78h88M42 38v40' />
        <path className={styles.artAccent} d='M42 70l12-10 12 7 16-18 15 8 17-14' />
        <circle className={styles.artAccent} cx='114' cy='43' r='3' />
      </g>
      <g className={styles.artStateBars}>
        <path d='M42 78h88' />
        {CREATE_BARS.map((bar, index) => (
          <rect
            key={bar.x}
            className={styles.artBar}
            x={bar.x}
            y={76 - bar.height}
            width='7'
            height={bar.height}
            rx='2'
            style={{ '--bar-delay': `${index * 35}ms` } as React.CSSProperties}
          />
        ))}
      </g>
      <path className={styles.artFrameTrace} d={WINDOW_FRAME_PATH} pathLength={1} />
    </svg>
  );
}

export default FeatureCards;
