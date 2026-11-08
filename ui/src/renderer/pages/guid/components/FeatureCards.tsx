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
 * The illustrations use the approved 176x96 geometry: develop reads as code
 * connecting into a browser page, create as a demo page with a playable frame.
 * Only inner SVG groups move on hover; the card frame, copy and neighbours stay
 * put. Reduced motion keeps every group stationary.
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

/** Build: code block -> arrow -> browser page; only the inner groups move. */
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
      <g className={styles.artSource}>
        <rect x='9' y='29' width='47' height='38' rx='7' />
        <path d='m24 43-6 5 6 5m13-10 6 5-6 5m-5-13-3 16' />
      </g>
      <path className={styles.artLink} d='M60 48h18m-5-5 5 5-5 5' />
      <rect x='84' y='17' width='82' height='62' rx='8' />
      <path d='M84 31h82M94 24h2m5 0h2' />
      <g className={styles.artResult}>
        <rect x='95' y='41' width='59' height='12' rx='3' className={styles.artAccent} />
        <path d='M95 63h24m8 0h27M95 70h18m14 0h18' />
      </g>
    </svg>
  );
}

/** Create: chart demo page plus a playable frame with a play key and timeline. */
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
      <g className={styles.artSource}>
        <rect x='12' y='13' width='78' height='55' rx='7' />
        <path d='M24 26h42m-42 9h23' />
        <path d='M25 55v-8h8v8m5 0V44h8v11m5 0V41h8v14' />
      </g>
      <g className={styles.artResult}>
        <rect className={styles.artPlate} x='74' y='32' width='87' height='51' rx='8' />
        <path className={styles.artAccent} d='m110 45 14 8-14 8Z' />
        <path d='M86 71h63' />
        <circle className={styles.artAccent} cx='102' cy='71' r='3' />
      </g>
    </svg>
  );
}

export default FeatureCards;
