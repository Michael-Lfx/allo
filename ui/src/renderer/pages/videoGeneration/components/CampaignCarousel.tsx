
import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { openExternalUrl } from '@renderer/utils/platform';
import { listCampaignCarousel } from '../api';
import {
  campaignCarouselAction,
  inAppNavigatePath,
  isHttpUrl,
  isInAppCampaignPath,
} from '../campaign';
import type { CampaignCarouselItem } from '../types';
import styles from '../campaign.module.css';

const ROTATE_MS = 4500;

const CampaignCarousel: React.FC = () => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const location = useLocation();
  const { status: cloudStatus } = useCloudAuth();
  const [items, setItems] = useState<CampaignCarouselItem[]>([]);
  const [index, setIndex] = useState(0);
  const [hoverPaused, setHoverPaused] = useState(false);
  const [inView, setInView] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const videoRefs = useRef<Array<HTMLVideoElement | null>>([]);

  useEffect(() => {
    if (cloudStatus !== 'authenticated') {
      setItems([]);
      return;
    }
    let cancelled = false;
    void listCampaignCarousel()
      .then((list) => {
        if (!cancelled) setItems(list);
      })
      .catch((error) => {
        console.warn('[videoGeneration] campaign carousel skipped', error);
        if (!cancelled) setItems([]);
      });
    return () => {
      cancelled = true;
    };
  }, [cloudStatus]);

  useEffect(() => {
    const node = rootRef.current;
    if (!node) return;
    const io = new IntersectionObserver(
      ([entry]) => {
        setInView(Boolean(entry?.isIntersecting && (entry.intersectionRatio ?? 0) >= 0.55));
      },
      { threshold: [0, 0.55, 1] }
    );
    io.observe(node);
    return () => io.disconnect();
  }, [items.length]);

  useEffect(() => {
    videoRefs.current.forEach((video, i) => {
      if (!video) return;
      if (i === index && inView) {
        void video.play().catch(() => undefined);
      } else {
        video.pause();
      }
    });
  }, [index, inView, items]);

  useEffect(() => {
    if (items.length <= 1) return;
    const reduced = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
    if (reduced) return;
    const timer = window.setInterval(() => {
      if (hoverPaused || !inView) return;
      if (document.visibilityState !== 'visible') return;
      setIndex((prev) => (prev + 1) % items.length);
    }, ROTATE_MS);
    return () => window.clearInterval(timer);
  }, [hoverPaused, inView, items.length]);

  const openItem = useCallback(
    (item: CampaignCarouselItem) => {
      const action = campaignCarouselAction(item);
      if (action === 'detail') {
        navigate(`/video-generation/campaigns/${item.id}`, {
          state: { fromSearch: location.search },
        });
        return;
      }
      if (action !== 'link' || !item.linkUrl) return;
      const url = item.linkUrl.trim();
      if (isInAppCampaignPath(url)) {
        navigate(inAppNavigatePath(url));
        return;
      }
      if (isHttpUrl(url)) {
        void openExternalUrl(url).catch((error) => {
          console.warn('[videoGeneration] campaign link failed', error);
        });
      }
    },
    [location.search, navigate]
  );

  if (cloudStatus !== 'authenticated' || items.length === 0) return null;

  return (
    <div
      ref={rootRef}
      className={styles.carousel}
      onMouseEnter={() => setHoverPaused(true)}
      onMouseLeave={() => setHoverPaused(false)}
    >
      <div className={styles.carouselViewport}>
        <div
          className={styles.carouselTrack}
          style={{ transform: `translateX(-${index * 100}%)` }}
        >
          {items.map((item, i) => {
            const clickable = campaignCarouselAction(item) !== 'none';
            return (
              <button
                key={item.id}
                type='button'
                className={`${styles.carouselSlide} ${
                  clickable ? styles.carouselSlideClickable : ''
                }`}
                disabled={!clickable}
                aria-label={item.title}
                onClick={() => openItem(item)}
              >
                {item.mediaType === 'video' ? (
                  <video
                    ref={(el) => {
                      videoRefs.current[i] = el;
                    }}
                    className={styles.carouselVideo}
                    src={item.mediaUrl}
                    poster={item.posterUrl || undefined}
                    muted
                    loop
                    playsInline
                    preload={i === index ? 'metadata' : 'none'}
                  />
                ) : (
                  <img
                    className={styles.carouselMedia}
                    src={item.mediaUrl}
                    alt=''
                    draggable={false}
                  />
                )}
                <div className={styles.carouselGradient} />
                {item.title ? <div className={styles.carouselTitle}>{item.title}</div> : null}
              </button>
            );
          })}
        </div>
        {items.length > 1 ? (
          <div className={styles.carouselDots} role='tablist' aria-label={t('videoGeneration.campaign.carouselDots', { defaultValue: '活动轮播' })}>
            {items.map((item, i) => (
              <button
                key={item.id}
                type='button'
                role='tab'
                aria-selected={i === index}
                className={`${styles.carouselDot} ${i === index ? styles.carouselDotActive : ''}`}
                onClick={(event) => {
                  event.stopPropagation();
                  setIndex(i);
                }}
              />
            ))}
          </div>
        ) : null}
      </div>
    </div>
  );
};

export default CampaignCarousel;
