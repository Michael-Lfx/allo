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
 *
 * Illustrations are purpose-drawn inline SVGs with distinct silhouettes:
 * develop = two code trails feeding a front-facing browser layout; create =
 * a diagonal film strip with sprocket holes over the corner of a demo page.
 * Only inner groups move on hover; the card frame, text and neighbours stay put.
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

/** Develop: code trails from the lower-left converge into a browser layout. */
function DevelopSketch() {
  return (
    <svg className={styles.sketch} viewBox='0 0 160 96' aria-hidden='true' focusable='false'>
      <g className={styles.pageGroup}>
        <rect className={styles.lineMain} x='54' y='14' width='92' height='62' rx='7' />
        <path className={styles.lineSub} d='M54 28h92' />
        <circle className={styles.dotSub} cx='62' cy='21' r='1.4' />
        <circle className={styles.dotSub} cx='68' cy='21' r='1.4' />
        <circle className={styles.dotSub} cx='74' cy='21' r='1.4' />
        <path className={styles.lineSub} d='M92 28v48' />
        <rect className={styles.lineSub} x='60' y='36' width='24' height='34' rx='2' />
        <rect className={styles.lineSub} x='100' y='36' width='38' height='14' rx='2' />
        <rect className={styles.lineSub} x='100' y='56' width='38' height='14' rx='2' />
      </g>
      <g className={styles.codeGroup}>
        <path className={styles.lineSub} d='M8 84l4 4-4 4' />
        <path className={styles.lineSub} d='M18 78l4 4-4 4' />
        <path className={styles.lineSub} d='M14 88h16l12-12h12' />
        <path className={styles.lineSub} d='M24 82h10l12-10h8' />
        <path className={styles.trailLight} d='M44 76h10' />
        <circle className={styles.node} cx='54' cy='76' r='2.4' />
      </g>
    </svg>
  );
}

/** Create: diagonal film strip with sprocket holes over a demo page corner. */
function CreateSketch() {
  return (
    <svg className={styles.sketch} viewBox='0 0 160 96' aria-hidden='true' focusable='false'>
      <g className={styles.backSheet}>
        <rect className={styles.lineSub} x='58' y='12' width='86' height='54' rx='6' />
        <path className={styles.lineSub} d='M68 24h34' />
        <path className={styles.lineSub} d='M68 32h24' />
      </g>
      <g className={styles.filmRotate} transform='rotate(-12 88 58)'>
        <g className={styles.filmBody}>
          <rect className={styles.lineMain} x='42' y='42' width='94' height='34' rx='6' />
          <rect className={styles.holeSub} x='50' y='47' width='9' height='5' rx='1.6' />
          <rect className={styles.holeAccent} x='68' y='47' width='9' height='5' rx='1.6' />
          <rect className={styles.holeSub} x='86' y='47' width='9' height='5' rx='1.6' />
          <path className={styles.playTriangle} d='M96 51l14 8-14 8z' />
          <path className={styles.lineSub} d='M50 68h22' />
          <path className={styles.lineSub} d='M120 66h10' />
        </g>
      </g>
      <circle className={styles.nodeWarm} cx='132' cy='82' r='2.4' />
    </svg>
  );
}

export default FeatureCards;
