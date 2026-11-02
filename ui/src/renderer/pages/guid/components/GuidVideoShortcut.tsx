import React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import videoCover from './assets/showcase-video-cover.png';
import styles from './GuidVideoShortcut.module.css';

/** Homepage shortcut into the video tab, matching the corner promo chip. */
const GuidVideoShortcut: React.FC = () => {
  const { t } = useTranslation();
  const navigate = useNavigate();

  return (
    <button
      type='button'
      className={styles.chip}
      data-button-shape='pill'
      data-testid='guid-home-video-shortcut'
      onClick={() => navigate('/video-generation')}
    >
      <span className={styles.thumb}>
        <img className={styles.cover} src={videoCover} alt='' />
        <span className={styles.play} aria-hidden='true'>
          <svg viewBox='0 0 12 12'>
            <path d='M4.2 2.4v7.2L10 6Z' fill='currentColor' />
          </svg>
        </span>
      </span>
      <span className={styles.copy}>
        <span className={styles.title}>{t('guid.home.videoShortcutTitle')}</span>
        <span className={styles.subtitle}>{t('guid.home.videoShortcutSubtitle')}</span>
      </span>
    </button>
  );
};

export default GuidVideoShortcut;
