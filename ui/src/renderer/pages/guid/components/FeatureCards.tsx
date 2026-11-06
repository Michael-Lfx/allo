import React from 'react';
import { useTranslation } from 'react-i18next';
import styles from './FeatureCards.module.css';

export interface FeatureCardsProps {
  /** Opens the develop directory (new project / existing project). */
  onOpenDevelop: () => void;
  /** Opens the create directory (slides / video). */
  onOpenCreate: () => void;
}

/**
 * Home stage entries: two stacked capability buttons (develop / create).
 * The illustrations are purpose-drawn inline SVGs: a front-facing browser with
 * a code symbol for develop, a front-facing play frame over a demo page for
 * create. Both are decorative (`aria-hidden`) and theme-adaptive; only the
 * inner group moves on hover, the card frame stays put.
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
        <DevelopSketch />
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
        <CreateSketch />
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

function DevelopSketch() {
  return (
    <svg className={styles.sketch} viewBox='0 0 160 96' aria-hidden='true' focusable='false'>
      <g className={styles.sketchFloat}>
        <g className={styles.codeGroup}>
          <rect className={styles.lineMain} x='30' y='18' width='108' height='62' rx='7' />
          <path className={styles.lineSub} d='M30 34h108' />
          <circle className={styles.dotSub} cx='41' cy='26' r='1.5' />
          <circle className={styles.dotSub} cx='48' cy='26' r='1.5' />
          <circle className={styles.dotSub} cx='55' cy='26' r='1.5' />
          <path className={styles.lineMain} d='M76 49l-8 9 8 9' />
          <path className={styles.lineMain} d='M94 49l8 9-8 9' />
          <path className={styles.lineSub} d='M88 46l-7 22' />
        </g>
        <path className={styles.lineSub} d='M36 80 16 90' />
        <circle className={styles.node} cx='16' cy='90' r='2.4' />
      </g>
    </svg>
  );
}

function CreateSketch() {
  return (
    <svg className={styles.sketch} viewBox='0 0 160 96' aria-hidden='true' focusable='false'>
      <g className={styles.sketchFloat}>
        <g className={styles.backSheet}>
          <rect className={styles.lineSub} x='44' y='12' width='92' height='52' rx='6' />
          <path className={styles.lineSub} d='M54 24h36' />
          <path className={styles.lineSub} d='M54 32h26' />
        </g>
        <g className={styles.playGroup}>
          <rect className={styles.lineMain} x='44' y='24' width='96' height='54' rx='7' />
          <path className={styles.lineMain} d='M86 44l15 9-15 9z' />
        </g>
        <path className={styles.lineAccent} d='M42 82c-6 2-12 1-18-2' />
        <circle className={styles.node} cx='22' cy='82' r='2.4' />
      </g>
    </svg>
  );
}

export default FeatureCards;
