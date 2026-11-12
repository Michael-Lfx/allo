import React, { useCallback, useEffect, useRef, useState } from 'react';
import { Modal } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import { Close, Left, Play, Right } from '@icon-park/react';
import { listTvShow } from '../../videoGeneration/api';
import type { TvShowVideo } from '../../videoGeneration/types';
import styles from './VideoExamplePanel.module.css';

const PAGE_SIZE = 15;

type VideoExamplePanelProps = {
  onClose: () => void;
  onPreviewPlayingChange: (playing: boolean) => void;
};

/**
 * Homepage video example drawer. Opening it only reads the public plaza list;
 * it never touches the media-model catalog or the composer draft. A card click
 * opens a small preview instead of copying the work title into the prompt.
 */
const VideoExamplePanel: React.FC<VideoExamplePanelProps> = ({ onClose, onPreviewPlayingChange }) => {
  const { t } = useTranslation();
  const [videos, setVideos] = useState<TvShowVideo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [previewVideo, setPreviewVideo] = useState<TvShowVideo | null>(null);
  const [reloadToken, setReloadToken] = useState(0);
  const [edges, setEdges] = useState({ start: true, end: false });
  const railRef = useRef<HTMLDivElement>(null);

  const updateEdges = useCallback(() => {
    const rail = railRef.current;
    if (!rail) return;
    const start = rail.scrollLeft <= 1;
    const end = rail.scrollLeft + rail.clientWidth >= rail.scrollWidth - 1;
    setEdges((current) => (current.start === start && current.end === end ? current : { start, end }));
  }, []);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    void listTvShow({ page: 1, pageSize: PAGE_SIZE, sort: 'publishedAtDesc' })
      .then((result) => {
        if (!cancelled) setVideos(result.list ?? []);
      })
      .catch((cause) => {
        if (!cancelled) setError(cause instanceof Error ? cause.message : String(cause));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [reloadToken]);

  useEffect(() => {
    updateEdges();
    const rail = railRef.current;
    if (!rail || typeof ResizeObserver === 'undefined') return undefined;
    const observer = new ResizeObserver(updateEdges);
    observer.observe(rail);
    return () => observer.disconnect();
  }, [updateEdges, videos.length]);

  useEffect(() => () => onPreviewPlayingChange(false), [onPreviewPlayingChange]);

  const stopPreview = () => {
    onPreviewPlayingChange(false);
    setPreviewVideo(null);
  };

  const scrollRail = (direction: -1 | 1) => {
    const rail = railRef.current;
    if (!rail) return;
    rail.scrollBy({ left: direction * rail.clientWidth * 0.9, behavior: 'smooth' });
  };

  return (
    <section className={styles.panel} data-testid='guid-home-video-panel'>
      <header className={styles.header}>
        <div className={styles.headingCopy}>
          <h2 className={styles.heading}>{t('guid.home.videoPanelTitle')}</h2>
          <p className={styles.subheading}>{t('guid.home.videoPanelSubtitle')}</p>
        </div>
        <span className={styles.headerActions}>
          <button
            type='button'
            className={styles.iconButton}
            aria-label={t('presentation.create.previous')}
            disabled={edges.start}
            onClick={() => scrollRail(-1)}
            data-testid='guid-home-video-prev'
          >
            <Left theme='outline' size={16} fill='currentColor' />
          </button>
          <button
            type='button'
            className={styles.iconButton}
            aria-label={t('presentation.create.next')}
            disabled={edges.end}
            onClick={() => scrollRail(1)}
            data-testid='guid-home-video-next'
          >
            <Right theme='outline' size={16} fill='currentColor' />
          </button>
          <button
            type='button'
            className={styles.close}
            aria-label={t('common.close', { defaultValue: 'Close' })}
            onClick={onClose}
            data-testid='guid-home-video-close'
          >
            <Close theme='outline' size={16} />
          </button>
        </span>
      </header>

      {loading ? (
        <div className={styles.status} data-testid='guid-home-video-loading' aria-busy='true'>
          <span className={styles.skeleton} aria-hidden='true' />
          <span className={styles.skeleton} aria-hidden='true' />
          <span className={styles.skeleton} aria-hidden='true' />
        </div>
      ) : error ? (
        <div className={styles.status} role='status'>
          <span>{t('videoGeneration.list.loadError')}</span>
          <button
            type='button'
            className={styles.retry}
            onClick={() => setReloadToken((value) => value + 1)}
          >
            {t('common.retry', { defaultValue: 'Retry' })}
          </button>
        </div>
      ) : (
        <div className={styles.rail} ref={railRef} onScroll={updateEdges}>
          {videos.map((video) => (
            <button
              key={video.id}
              type='button'
              className={styles.card}
              data-testid={`guid-home-video-example-${video.id}`}
              aria-label={video.title || t('videoGeneration.list.untitled')}
              onClick={() => setPreviewVideo(video)}
            >
              {video.coverUrl ? (
                <img
                  className={styles.cover}
                  src={video.coverUrl}
                  alt=''
                  loading='lazy'
                  decoding='async'
                  draggable={false}
                />
              ) : (
                <span className={styles.coverFallback} aria-hidden='true'>
                  <Play theme='outline' size={20} />
                </span>
              )}
              <span className={styles.scrim} aria-hidden='true' />
              <span className={styles.copy}>
                <strong className={styles.title}>{video.title}</strong>
                {video.author?.name ? <span className={styles.author}>{video.author.name}</span> : null}
              </span>
              <span className={styles.play} aria-hidden='true'>
                <Play theme='filled' size={16} />
              </span>
            </button>
          ))}
        </div>
      )}

      <Modal
        visible={previewVideo !== null}
        title={previewVideo?.title || t('videoGeneration.tvShow.detail.title')}
        footer={null}
        unmountOnExit
        autoFocus={false}
        onCancel={stopPreview}
        style={{ width: 'min(860px, 92vw)' }}
      >
        {previewVideo ? (
          <div className={styles.preview} data-testid='guid-home-video-preview'>
            {previewVideo.previewUrl ? (
              <video
                key={previewVideo.previewUrl}
                src={previewVideo.previewUrl}
                poster={previewVideo.coverUrl || undefined}
                controls
                playsInline
                preload='metadata'
                onPlaying={() => onPreviewPlayingChange(true)}
                onPause={() => onPreviewPlayingChange(false)}
                onEnded={() => onPreviewPlayingChange(false)}
                onError={() => onPreviewPlayingChange(false)}
                className={styles.previewMedia}
              />
            ) : previewVideo.coverUrl ? (
              <img
                src={previewVideo.coverUrl}
                alt=''
                className={styles.previewMedia}
                draggable={false}
              />
            ) : null}
            <div className={styles.previewMeta}>
              {previewVideo.author?.name ? <span>{previewVideo.author.name}</span> : null}
              <span>{t('guid.home.videoPreviewHint')}</span>
            </div>
          </div>
        ) : null}
      </Modal>
    </section>
  );
};

export default VideoExamplePanel;
