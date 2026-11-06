import React from 'react';
import { useTranslation } from 'react-i18next';
import appCover from './assets/showcase-app-cover.jpg';
import imageCover from './assets/showcase-image-cover.jpg';
import videoCover from './assets/showcase-video-cover.png';
import styles from './ShowcaseCards.module.css';

export interface ShowcaseCardsProps {
  onSelectVideo: () => void;
  onSelectApp: () => void;
  onSelectImage: () => void;
}

const ShowcaseCards: React.FC<ShowcaseCardsProps> = ({ onSelectVideo, onSelectApp, onSelectImage }) => {
  const { t } = useTranslation();

  return (
    <div className={styles.row}>
      <button
        type="button"
        className={styles.prompt}
        data-testid="guid-home-showcase-video"
        onClick={onSelectVideo}
      >
        <article className={styles.card}>
          <img className={styles.cover} src={videoCover} alt="" />
          <span className={styles.badge}>
            <VideoGlyph />
            {t('guid.home.showcaseVideoBadge')}
          </span>
        </article>
        <p className={styles.caption}>{t('guid.home.showcaseVideoTitle')}</p>
      </button>
      <button type="button" className={styles.prompt} onClick={onSelectApp}>
        <article className={styles.card}>
          <img className={`${styles.cover} ${styles.coverTop}`} src={appCover} alt="" />
          <span className={styles.badge}>
            <AppGlyph />
            {t('guid.home.showcaseAppBadge')}
          </span>
        </article>
        <p className={styles.caption}>{t('guid.home.showcaseAppTitle')}</p>
      </button>
      <button type='button' className={styles.prompt} onClick={onSelectImage}>
        <article className={styles.card}>
          <img className={styles.cover} src={imageCover} alt='' />
          <span className={styles.badge}>
            <ImageGlyph />
            {t('guid.home.showcaseImageBadge')}
          </span>
        </article>
        <p className={styles.caption}>{t('guid.home.showcaseImageTitle')}</p>
      </button>
    </div>
  );
};

function VideoGlyph() {
  return (
    <svg viewBox="0 0 16 16" aria-hidden="true">
      <rect x="2" y="4" width="8" height="8" rx="1.5" fill="none" stroke="currentColor" strokeWidth="1.3" />
      <path d="M10 6.5 14 4.5 V11.5 L10 9.5 Z" fill="currentColor" />
    </svg>
  );
}

function AppGlyph() {
  return (
    <svg viewBox="0 0 16 16" aria-hidden="true">
      <rect x="3" y="2.5" width="10" height="11" rx="1.6" fill="none" stroke="currentColor" strokeWidth="1.3" />
      <path d="M6.5 12.2 H9.5" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
    </svg>
  );
}

function ImageGlyph() {
  return (
    <svg viewBox="0 0 16 16" aria-hidden="true">
      <rect x="2" y="3" width="12" height="10" rx="1.6" fill="none" stroke="currentColor" strokeWidth="1.3" />
      <circle cx="6" cy="6.5" r="1.1" fill="currentColor" />
      <path d="M3.5 11.2 6.4 8.4 8.6 10.2 10.4 8.2 12.6 11.2" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" />
    </svg>
  );
}

export default ShowcaseCards;
