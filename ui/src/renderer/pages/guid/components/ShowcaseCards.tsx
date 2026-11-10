import React from 'react';
import { useTranslation } from 'react-i18next';
import websiteCover from './assets/build/website-main.webp';
import imageCover from './assets/showcase-image-cover.webp';
import videoCover from './assets/showcase-video-cover.webp';
import styles from './ShowcaseCards.module.css';

export interface ShowcaseCardsProps {
  onSelectVideo: () => void;
  onSelectWebsite: () => void;
  onSelectImage: () => void;
}

const ShowcaseCards: React.FC<ShowcaseCardsProps> = ({ onSelectVideo, onSelectWebsite, onSelectImage }) => {
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
      <button
        type="button"
        className={styles.prompt}
        data-testid="guid-home-showcase-website"
        onClick={onSelectWebsite}
      >
        <article className={styles.card}>
          <img className={styles.cover} src={websiteCover} alt="" />
          <span className={styles.badge}>
            <WebsiteGlyph />
            {t('guid.home.showcaseWebsiteBadge')}
          </span>
        </article>
        <p className={styles.caption}>{t('guid.home.showcaseWebsiteTitle')}</p>
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

function WebsiteGlyph() {
  return (
    <svg viewBox="0 0 16 16" aria-hidden="true">
      <rect x="2" y="3" width="12" height="10" rx="1.6" fill="none" stroke="currentColor" strokeWidth="1.3" />
      <path d="M2 6.2 H14" stroke="currentColor" strokeWidth="1.2" />
      <circle cx="4.2" cy="4.6" r="0.7" fill="currentColor" />
      <path d="M7.4 8.5 H11.8 M7.4 10.8 H10.2" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
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
