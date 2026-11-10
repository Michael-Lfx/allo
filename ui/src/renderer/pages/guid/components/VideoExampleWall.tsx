import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Play, Refresh, Right, VideoOne } from '@icon-park/react';
import { listTvShow } from '../../videoGeneration/api';
import type { TvShowVideo } from '../../videoGeneration/types';
import styles from './VideoExampleWall.module.css';

const PAGE_SIZE = 30;
const MAX_PAGES = 20;
/** The homepage is an example wall, not the full catalogue: bound both the
 * number of sequential API calls and the number of mounted cards. */
const MAX_EXAMPLES = 120;
const CACHE_TTL_MS = 60_000;
const SKELETON_ITEMS = Array.from({ length: 12 }, (_, index) => index);

type ExampleCache = { items: TvShowVideo[]; expiresAt: number };
type ExampleProgress = (items: TvShowVideo[]) => void;
let cachedExamples: ExampleCache | null = null;
let pendingExamples: Promise<TvShowVideo[]> | null = null;
const pendingProgress = new Set<ExampleProgress>();

async function fetchAllExamples(onProgress?: ExampleProgress): Promise<TvShowVideo[]> {
  const collected: TvShowVideo[] = [];
  const seen = new Set<number>();
  let page = 1;
  let total = 0;

  while (page <= MAX_PAGES && collected.length < MAX_EXAMPLES) {
    const result = await listTvShow({
      page,
      pageSize: PAGE_SIZE,
      sort: 'publishedAtDesc',
    });
    const batch = result.list ?? [];
    if (batch.length === 0) break;
    for (const video of batch) {
      if (seen.has(video.id)) continue;
      seen.add(video.id);
      collected.push(video);
      if (collected.length >= MAX_EXAMPLES) break;
    }
    total = result.total ?? collected.length;
    onProgress?.(collected.slice());
    if (collected.length >= total || collected.length >= MAX_EXAMPLES) break;
    page += 1;
  }

  return collected;
}

async function loadExamples(force = false, onProgress?: ExampleProgress): Promise<TvShowVideo[]> {
  if (!force && cachedExamples && cachedExamples.expiresAt > Date.now()) {
    onProgress?.(cachedExamples.items);
    return cachedExamples.items;
  }
  if (pendingExamples) {
    if (onProgress) pendingProgress.add(onProgress);
    try {
      const examples = await pendingExamples;
      onProgress?.(examples);
      return examples;
    } finally {
      if (onProgress) pendingProgress.delete(onProgress);
    }
  }
  if (onProgress) pendingProgress.add(onProgress);
  pendingExamples = fetchAllExamples((items) => {
    for (const listener of pendingProgress) listener(items);
  })
    .then((examples) => {
      cachedExamples = {
        items: examples,
        expiresAt: Date.now() + CACHE_TTL_MS,
      };
      return examples;
    })
    .finally(() => {
      pendingExamples = null;
    });
  try {
    const examples = await pendingExamples;
    onProgress?.(examples);
    return examples;
  } finally {
    if (onProgress) pendingProgress.delete(onProgress);
  }
}

type VideoExampleCardProps = {
  video: TvShowVideo;
  onSelect: (video: TvShowVideo) => void;
};

const VideoExampleCard: React.FC<VideoExampleCardProps> = ({ video, onSelect }) => {
  const { t } = useTranslation();
  const videoRef = useRef<HTMLVideoElement>(null);
  const [previewing, setPreviewing] = useState(false);
  const previewUrl = video.previewUrl?.trim() || '';

  useEffect(() => {
    if (!previewing) return;
    const element = videoRef.current;
    if (!element) return;
    element.currentTime = 0;
    void element.play().catch(() => undefined);
    return () => element.pause();
  }, [previewing]);

  const handleEnter = useCallback(() => {
    if (!previewUrl) return;
    // Hover previews are motion: respect the user's reduced-motion setting.
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    setPreviewing(true);
  }, [previewUrl]);

  const handleLeave = useCallback(() => {
    setPreviewing(false);
  }, []);

  return (
    <button
      type='button'
      className={styles.tile}
      data-testid='guid-home-video-example'
      aria-label={video.title?.trim() || t('videoGeneration.list.untitled')}
      title={video.title}
      onClick={() => onSelect(video)}
      onMouseEnter={handleEnter}
      onMouseLeave={handleLeave}
    >
      <span className={styles.media}>
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
          <span className={styles.fallback}>
            <VideoOne theme='outline' size={22} />
          </span>
        )}
        {previewing && previewUrl ? (
          <video
            ref={videoRef}
            className={styles.preview}
            src={previewUrl}
            poster={video.coverUrl || undefined}
            muted
            loop
            playsInline
            preload='metadata'
          />
        ) : null}
      </span>
      <span className={styles.shade} aria-hidden='true' />
      <span className={styles.copy}>
        <strong className={styles.title}>{video.title}</strong>
        {video.author?.name ? <span className={styles.author}>{video.author.name}</span> : null}
      </span>
      <span className={styles.play} aria-hidden='true'>
        <Play theme='filled' size={16} />
      </span>
    </button>
  );
};

type VideoExampleWallProps = {
  onSelect: (video: TvShowVideo) => void;
  /** Opens the full video plaza when the homepage preview is bounded. */
  onOpenAll?: () => void;
};

/**
 * Homepage video capability gallery. It reads the same Flowy TV plaza data as
 * `/video-generation` and turns each cover into a prompt starter, so the
 * homepage is not a single decorative hero.
 */
const VideoExampleWall: React.FC<VideoExampleWallProps> = ({ onSelect, onOpenAll }) => {
  const { t } = useTranslation();
  const [videos, setVideos] = useState<TvShowVideo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const requestIdRef = useRef(0);

  const load = useCallback(async (force = false) => {
    const requestId = requestIdRef.current + 1;
    requestIdRef.current = requestId;
    setLoading(true);
    setError(null);
    try {
      const examples = await loadExamples(force, (partial) => {
        if (requestIdRef.current !== requestId) return;
        setVideos(partial);
        setLoading(false);
      });
      if (requestIdRef.current === requestId) setVideos(examples);
    } catch (cause) {
      if (requestIdRef.current === requestId) {
        setError(cause instanceof Error ? cause.message : String(cause));
      }
    } finally {
      if (requestIdRef.current === requestId) setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
    return () => {
      requestIdRef.current += 1;
    };
  }, [load]);

  return (
    <section className={styles.wall} data-testid='guid-home-video-examples'>
      <header className={styles.header}>
        <div>
          <h2 className={styles.heading}>{t('videoGeneration.tvShow.title')}</h2>
          <p className={styles.subheading}>{t('guid.home.videoExamplesSubtitle')}</p>
        </div>
        {videos.length > 0 ? (
          <span className={styles.headerActions}>
            <span className={styles.count}>{videos.length}</span>
            {onOpenAll ? (
              <button type='button' className={styles.viewAll} onClick={onOpenAll}>
                <span>{t('guid.home.videoExamplesViewAll')}</span>
                <Right theme='outline' size={13} />
              </button>
            ) : null}
          </span>
        ) : null}
      </header>
      {loading && videos.length === 0 ? (
        <div className={styles.masonry} aria-busy='true' data-testid='guid-home-video-examples-loading'>
          {SKELETON_ITEMS.map((item) => (
            <span key={item} className={styles.skeleton} aria-hidden='true' />
          ))}
        </div>
      ) : null}
      {!loading && error && videos.length === 0 ? (
        <div className={styles.status} role='status'>
          <span>{t('videoGeneration.list.loadError')}</span>
          <button type='button' className={styles.retry} onClick={() => void load(true)}>
            <Refresh theme='outline' size={13} />
            <span>{t('common.retry')}</span>
          </button>
        </div>
      ) : null}
      {videos.length > 0 ? (
        <>
          <div className={styles.masonry}>
            {videos.map((video) => (
              <VideoExampleCard key={video.id} video={video} onSelect={onSelect} />
            ))}
          </div>
          {error ? (
            <div className={styles.status} role='status'>
              <span>{t('videoGeneration.list.loadError')}</span>
              <button type='button' className={styles.retry} onClick={() => void load(true)}>
                <Refresh theme='outline' size={13} />
                <span>{t('common.retry')}</span>
              </button>
            </div>
          ) : null}
        </>
      ) : null}
    </section>
  );
};

export default VideoExampleWall;
