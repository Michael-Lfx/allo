import React, { useCallback, useContext, useEffect, useMemo, useState } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { openExternalUrl } from '@renderer/utils/platform';
import { ThemeContext } from '@renderer/hooks/context/ThemeContext';
import { listCampaignCarousel } from '../api';
import {
  campaignCarouselAction,
  campaignCarouselCard,
  inAppNavigatePath,
  isHttpUrl,
  isInAppCampaignPath,
  pickCampaignLocalizedText,
} from '../campaign';
import type { CampaignCarouselItem } from '../types';
import CircularCarousel, { type CircularCarouselItem } from './CircularCarousel';
import styles from '../campaign.module.css';

const CampaignCarousel: React.FC = () => {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const location = useLocation();
  const theme = useContext(ThemeContext);
  const [items, setItems] = useState<CampaignCarouselItem[]>([]);
  const [fadeColor, setFadeColor] = useState(theme?.theme === 'dark' ? '#141418' : '#f6f7f8');

  // useEffect必要性：活动轮播接口；目的：拉取输入框上方的活动封面。
  // 未使用 ahooks：只在挂载时请求一次，失败时清空而不是重试弹错。
  useEffect(() => {
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
  }, []);

  // useEffect必要性：页面背景色；目的：让远处的卡片淡入当前背景，而不是一块纯黑。
  useEffect(() => {
    const probe = document.querySelector('.app-shell') ?? document.body;
    const color = getComputedStyle(probe).backgroundColor;
    if (color && color !== 'rgba(0, 0, 0, 0)' && color !== 'transparent') {
      setFadeColor(color);
      return;
    }
    setFadeColor(theme?.theme === 'dark' ? '#141418' : '#f6f7f8');
  }, [theme?.theme]);

  const slides = useMemo<CircularCarouselItem[]>(
    () =>
      items.map((item) => {
        const title = pickCampaignLocalizedText(item.title, item.titleEn, i18n.language);
        const video = item.mediaType === 'video';
        const poster = item.posterUrl?.trim() || '';
        return {
          src: video ? poster || item.mediaUrl : item.mediaUrl,
          videoSrc: video ? item.mediaUrl : undefined,
          alt: title,
          title,
        };
      }),
    [i18n.language, items]
  );

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

  if (items.length === 0) return null;

  const card = campaignCarouselCard(slides.length);

  return (
    <div className={styles.carousel}>
      <CircularCarousel
        items={slides}
        preset='cylinder'
        intro='rise'
        cardWidth={card.cardWidth}
        aspectRatio={card.aspectRatio}
        gap={card.gap}
        curve={0}
        speed={11}
        captions
        fadeColor={fadeColor}
        depthFade={0.62}
        autoplay={slides.length > 1 ? 'drift' : 'off'}
        ariaLabel={t('videoGeneration.campaign.carouselDots', { defaultValue: '活动轮播' })}
        onItemClick={(_item, index) => {
          const item = items[index];
          if (item) openItem(item);
        }}
      />
    </div>
  );
};

export default CampaignCarousel;
