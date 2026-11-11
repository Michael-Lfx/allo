import React from 'react';
import { useTranslation } from 'react-i18next';
import styles from './FeatureCards.module.css';

export interface FeatureCardsProps {
  onStartFromLocal: () => void;
  /** "Create" card: opens the slides panel. Omitted keeps the card static. */
  onCreate?: () => void;
}

const FeatureCards: React.FC<FeatureCardsProps> = ({ onStartFromLocal, onCreate }) => {
  const { t } = useTranslation();

  return (
    <div className={styles.board}>
      <article className={`${styles.entry} ${styles.build}`}>
        <BuildMark />
        <h2 className={styles.entryTitle}>
          {t('guid.home.buildTitle')}
          <Chevron />
        </h2>
        <p className={styles.entryDescription}>{t('guid.home.buildDescription')}</p>
        <BuildSketch />
      </article>

      {onCreate ? (
        <button
          type='button'
          data-testid='guid-home-create'
          className={`${styles.entry} ${styles.create} ${styles.entryButton}`}
          onClick={onCreate}
        >
          <CreateMark />
          <span className={styles.entryTitle}>
            {t('guid.home.createTitle')}
            <Chevron />
          </span>
          <span className={styles.entryDescription}>{t('guid.home.createDescription')}</span>
          <CreateSketch />
        </button>
      ) : (
        <article className={`${styles.entry} ${styles.create}`}>
          <CreateMark />
          <h2 className={styles.entryTitle}>
            {t('guid.home.createTitle')}
            <Chevron />
          </h2>
          <p className={styles.entryDescription}>{t('guid.home.createDescription')}</p>
          <CreateSketch />
        </article>
      )}

      <button type='button' data-testid='guid-home-studio' className={styles.studio} onClick={onStartFromLocal}>
        <FolderSketch />
        <h2 className={styles.studioTitle}>{t('guid.home.studioTitle')}</h2>
      </button>
    </div>
  );
};

function Chevron() {
  return (
    <svg className={styles.chevron} viewBox="0 0 16 16" aria-hidden="true">
      <path d="M6 3.5 10.5 8 6 12.5" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function BuildMark() {
  return (
    <svg className={styles.mark} viewBox="0 0 24 24" aria-hidden="true">
      <path d="M6 5.5 17.5 11 11 12.2 9.2 18.5 6 5.5Z" fill="#4c8dff" />
    </svg>
  );
}

function CreateMark() {
  return (
    <svg className={styles.mark} viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="8" cy="14" r="2.2" fill="#ff9a3c" />
      <circle cx="12" cy="8.2" r="2.2" fill="#ffb15a" />
      <circle cx="16" cy="14" r="2.2" fill="#ff7a1a" />
    </svg>
  );
}

function BuildSketch() {
  return (
    <svg className={styles.sketch} viewBox="0 0 240 150" aria-hidden="true">
      <g className={styles.sketchFloat}>
        <g fill="none" stroke="#d7dbe2" strokeWidth="1">
          <path d="M36 118 H206" />
          <path d="M58 108 L176 49" />
          <path d="M92 124 L198 71" />
        </g>
        <g className={styles.slabSides}>
          <path d="M86 74 L142 102 V124 L86 96 Z" fill="#f4f6f8" stroke="#c5cbd3" strokeWidth="1.15" strokeLinejoin="round" />
          <path d="M142 102 L214 66 V88 L142 124 Z" fill="#e7ebf0" stroke="#c5cbd3" strokeWidth="1.15" strokeLinejoin="round" />
        </g>
        <g className={styles.slabTop}>
          <path d="M86 52 L158 16 L214 44 L142 80 Z" fill="#ffffff" stroke="#b7bec7" strokeWidth="1.2" strokeLinejoin="round" />
          <path d="M108 49 L156 25 L164 29 L116 53 Z" fill="#4c8dff" />
          <path d="M112 60 L176 28" fill="none" stroke="#d5dae1" strokeWidth="1.15" strokeLinecap="round" />
          <path d="M120 68 L168 44" fill="none" stroke="#e1e5eb" strokeWidth="1.15" strokeLinecap="round" />
          <path d="M128 78 L160 62" fill="none" stroke="#e7ebf0" strokeWidth="1.15" strokeLinecap="round" />
        </g>
      </g>
    </svg>
  );
}

function CreateSketch() {
  return (
    <svg className={styles.sketch} viewBox="0 0 240 150" aria-hidden="true">
      <g className={styles.sketchFloat}>
        <path d="M62 86 L134 50 L186 76 L114 112 Z" fill="#f3f5f7" stroke="#d0d5dc" strokeWidth="1.15" strokeLinejoin="round" />
        <path className={styles.sheetMid} d="M78 78 L150 42 L202 68 L130 104 Z" fill="#fbfcfd" stroke="#c5cbd3" strokeWidth="1.15" strokeLinejoin="round" />
        <g className={styles.sheetFront}>
          <path d="M94 70 L166 34 L218 60 L146 96 Z" fill="#ffffff" stroke="#b7bec7" strokeWidth="1.2" strokeLinejoin="round" />
          <path d="M112 64 L154 43 L160 46 L118 67 Z" fill="#ff9a3c" />
          <path d="M118 74 L170 48" fill="none" stroke="#d5dae1" strokeWidth="1.15" strokeLinecap="round" />
          <path d="M124 82 L160 64" fill="none" stroke="#e4e8ed" strokeWidth="1.15" strokeLinecap="round" />
        </g>
      </g>
    </svg>
  );
}

function FolderSketch() {
  return (
    <svg className={styles.folder} viewBox="0 0 160 120" aria-hidden="true">
      <g className={styles.sketchFloat}>
        <path
          d="M30 50 H62 L72 60 H124 a10 10 0 0 1 10 10 V74 H22 V60 a10 10 0 0 1 8 -10 Z"
          fill="#e4e8ee"
        />
        <g className={styles.folderSheets}>
          <rect x="48" y="42" width="64" height="38" rx="5" fill="#f7f8fa" stroke="#d5dae1" strokeWidth="1.2" />
          <rect x="52" y="48" width="56" height="34" rx="4" fill="#ffffff" stroke="#e1e5eb" strokeWidth="1.1" />
          <path d="M62 58 H96" fill="none" stroke="#e4e8ed" strokeWidth="1.4" strokeLinecap="round" />
          <path d="M62 65 H86" fill="none" stroke="#eef1f4" strokeWidth="1.4" strokeLinecap="round" />
        </g>
        <path
          d="M20 68 H140 a12 12 0 0 1 12 12 V102 a12 12 0 0 1 -12 12 H20 a12 12 0 0 1 -12 -12 V80 a12 12 0 0 1 12 -12 Z"
          fill="#fbfcfd"
          stroke="#c5cbd3"
          strokeWidth="1.4"
          strokeLinejoin="round"
        />
      </g>
    </svg>
  );
}

export default FeatureCards;
