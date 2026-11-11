import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Close, ImageFiles, Left, Right } from '@icon-park/react';
import cinematicPortrait from './assets/image-examples/cinematic-portrait.webp';
import premiumProduct from './assets/image-examples/premium-product.webp';
import storybookIsland from './assets/image-examples/storybook-island.webp';
import warmInterior from './assets/image-examples/warm-interior.webp';
import {
  DEFAULT_IMAGE_PROMPT_PARAMETERS,
  type ImagePromptParameters,
} from './imagePromptParameters';
import styles from './ImagePromptPanel.module.css';

/**
 * Image examples are seeded from the CC0-1.0 collection
 * Evolink-AI/awesome-gpt-image-2-API-Prompts:
 * Rainy Bus Stop Portrait, Luxury Amber Perfume Ad, Whimsical Mirror Travel
 * Journal and the cozy interior lifestyle case. Prompts are shortened and
 * rewritten as model-agnostic generation briefs.
 */
type ImagePromptPanelProps = {
  onSelectPrompt: (prompt: string, parameters: ImagePromptParameters) => void;
  onClose: () => void;
};

type ImageExample = {
  key: string;
  cover: string;
  titleKey: string;
  promptKey: string;
  parameters: ImagePromptParameters;
};

const IMAGE_EXAMPLES: ImageExample[] = [
  {
    key: 'cinematic',
    cover: cinematicPortrait,
    titleKey: 'guid.home.imageExampleCinematicTitle',
    promptKey: 'guid.home.imageExampleCinematicPrompt',
    parameters: { style: 'cinematic', aspect: '16:9', details: ['film', 'bokeh'] },
  },
  {
    key: 'product',
    cover: premiumProduct,
    titleKey: 'guid.home.imageExampleProductTitle',
    promptKey: 'guid.home.imageExampleProductPrompt',
    parameters: { style: 'product', aspect: '1:1', details: ['sharp', 'soft'] },
  },
  {
    key: 'illustration',
    cover: storybookIsland,
    titleKey: 'guid.home.imageExampleIllustrationTitle',
    promptKey: 'guid.home.imageExampleIllustrationPrompt',
    parameters: { style: 'illustration', aspect: '16:9', details: ['sharp', 'soft'] },
  },
  {
    key: 'interior',
    cover: warmInterior,
    titleKey: 'guid.home.imageExampleInteriorTitle',
    promptKey: 'guid.home.imageExampleInteriorPrompt',
    parameters: { style: 'interior', aspect: '16:9', details: ['sharp'] },
  },
];

const ImagePromptPanel: React.FC<ImagePromptPanelProps> = ({ onSelectPrompt, onClose }) => {
  const { t } = useTranslation();
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const railRef = useRef<HTMLDivElement>(null);
  const [edges, setEdges] = useState({ start: true, end: false });

  const updateEdges = useCallback(() => {
    const rail = railRef.current;
    if (!rail) return;
    const start = rail.scrollLeft <= 1;
    const end = rail.scrollLeft + rail.clientWidth >= rail.scrollWidth - 1;
    setEdges((current) => (current.start === start && current.end === end ? current : { start, end }));
  }, []);

  useEffect(() => {
    const rail = railRef.current;
    if (!rail) return;
    updateEdges();
    const observer = new ResizeObserver(updateEdges);
    observer.observe(rail);
    return () => observer.disconnect();
  }, [updateEdges]);

  const scrollRail = (direction: -1 | 1) => {
    const rail = railRef.current;
    if (!rail) return;
    rail.scrollBy({ left: direction * rail.clientWidth * 0.9, behavior: 'smooth' });
  };

  const select = (key: string, prompt: string, parameters: ImagePromptParameters) => {
    setSelectedKey(key);
    onSelectPrompt(prompt, parameters);
  };

  return (
    <section className={styles.panel} data-testid='guid-home-image-panel' aria-label={t('guid.home.imageTitle')}>
      <header className={styles.header}>
        <div className={styles.headingCopy}>
          <h2 className={styles.heading}>{t('guid.home.imageTitle')}</h2>
          <p className={styles.subheading}>{t('guid.home.imageSubtitle')}</p>
        </div>
        <span className={styles.headerActions}>
          <button
            type='button'
            className={styles.iconButton}
            aria-label={t('presentation.create.previous')}
            disabled={edges.start}
            onClick={() => scrollRail(-1)}
            data-testid='guid-home-image-prev'
          >
            <Left theme='outline' size={16} fill='currentColor' />
          </button>
          <button
            type='button'
            className={styles.iconButton}
            aria-label={t('presentation.create.next')}
            disabled={edges.end}
            onClick={() => scrollRail(1)}
            data-testid='guid-home-image-next'
          >
            <Right theme='outline' size={16} fill='currentColor' />
          </button>
          <button
            type='button'
            className={styles.close}
            data-testid='guid-home-image-close'
            onClick={onClose}
            aria-label={t('common.close', { defaultValue: 'Close' })}
          >
            <Close theme='outline' size={16} />
          </button>
        </span>
      </header>

      <div className={styles.rail} ref={railRef} onScroll={updateEdges}>
        <button
          type='button'
          className={`${styles.tile} ${styles.custom}`}
          data-testid='guid-home-image-custom'
          data-selected={selectedKey === 'custom' || undefined}
          onClick={() => select('custom', t('guid.home.imageCustomPrompt'), DEFAULT_IMAGE_PROMPT_PARAMETERS)}
        >
          <span className={styles.customIcon} aria-hidden='true'>
            <ImageFiles theme='outline' size={19} />
          </span>
          <span className={styles.customCopy}>
            <strong className={styles.customTitle}>{t('guid.home.imageCustomTitle')}</strong>
            <span className={styles.customHint}>{t('guid.home.imageTip')}</span>
          </span>
        </button>

        {IMAGE_EXAMPLES.map((example) => (
          <button
            key={example.key}
            type='button'
            className={styles.tile}
            data-testid={`guid-home-image-example-${example.key}`}
            data-selected={selectedKey === example.key || undefined}
            onClick={() => select(example.key, t(example.promptKey), example.parameters)}
          >
            <img className={styles.cover} src={example.cover} alt='' loading='lazy' decoding='async' />
            <span className={styles.scrim} aria-hidden='true' />
            <strong className={styles.exampleTitle}>{t(example.titleKey)}</strong>
          </button>
        ))}
      </div>
    </section>
  );
};

export default ImagePromptPanel;
