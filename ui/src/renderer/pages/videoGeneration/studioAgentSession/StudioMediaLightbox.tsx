import React, { useCallback, useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import { CloseSmall, Left, Right } from '@icon-park/react';
import { loadArtifactMediaUrlCached } from '../api';
import type { StudioSessionMedia } from './types';
import styles from './index.module.css';

export interface StudioMediaLightboxProps {
  sessionId: string;
  items: StudioSessionMedia[];
  index: number;
  onIndexChange: (index: number) => void;
  onClose: () => void;
}

const StudioMediaLightbox: React.FC<StudioMediaLightboxProps> = ({
  sessionId,
  items,
  index,
  onIndexChange,
  onClose,
}) => {
  const { t } = useTranslation();
  const total = items.length;
  const current = items[Math.max(0, Math.min(index, Math.max(0, total - 1)))];
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    if (!current) {
      setUrl(null);
      return;
    }
    let cancelled = false;
    setUrl(null);
    void loadArtifactMediaUrlCached(sessionId, current.path)
      .then((next) => {
        if (!cancelled) setUrl(next);
      })
      .catch(() => {
        if (!cancelled) setUrl(null);
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId, current?.path]);

  useEffect(() => {
    const neighbor = items[index + 1] ?? items[index - 1];
    if (!neighbor) return;
    void loadArtifactMediaUrlCached(sessionId, neighbor.path).catch(() => undefined);
  }, [sessionId, items, index]);

  const step = useCallback(
    (delta: number) => {
      if (total <= 1) return;
      onIndexChange((index + delta + total) % total);
    },
    [index, onIndexChange, total]
  );

  useEffect(() => {
    const prevOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
      else if (event.key === 'ArrowLeft') step(-1);
      else if (event.key === 'ArrowRight') step(1);
    };
    window.addEventListener('keydown', onKey);
    return () => {
      document.body.style.overflow = prevOverflow;
      window.removeEventListener('keydown', onKey);
    };
  }, [onClose, step]);

  if (!current || typeof document === 'undefined') return null;

  const label = current.label || current.path.split('/').pop() || '';

  return createPortal(
    <div
      className={styles.lightbox}
      role='dialog'
      aria-modal='true'
      aria-label={label}
      onClick={onClose}
    >
      <button
        type='button'
        className={`${styles.lightboxIconBtn} ${styles.lightboxClose}`}
        onClick={(event) => {
          event.stopPropagation();
          onClose();
        }}
        aria-label={t('videoGeneration.agentSession.preview.close', { defaultValue: '关闭预览' })}
      >
        <CloseSmall theme='outline' size={18} fill='currentColor' />
      </button>
      {total > 1 ? (
        <button
          type='button'
          className={`${styles.lightboxIconBtn} ${styles.lightboxNav} ${styles.lightboxNavLeft}`}
          onClick={(event) => {
            event.stopPropagation();
            step(-1);
          }}
          aria-label={t('videoGeneration.agentSession.preview.prev', { defaultValue: '上一张' })}
        >
          <Left theme='outline' size={18} fill='currentColor' />
        </button>
      ) : null}
      {total > 1 ? (
        <button
          type='button'
          className={`${styles.lightboxIconBtn} ${styles.lightboxNav} ${styles.lightboxNavRight}`}
          onClick={(event) => {
            event.stopPropagation();
            step(1);
          }}
          aria-label={t('videoGeneration.agentSession.preview.next', { defaultValue: '下一张' })}
        >
          <Right theme='outline' size={18} fill='currentColor' />
        </button>
      ) : null}
      <div
        className={styles.lightboxStage}
        onClick={(event) => event.stopPropagation()}
      >
        {url && current.kind === 'video' ? (
          <video
            key={url}
            className={styles.lightboxVideo}
            src={url}
            controls
            playsInline
            autoPlay
            muted
          />
        ) : url ? (
          <img className={styles.lightboxImg} src={url} alt={label} />
        ) : (
          <div className={styles.lightboxPending} />
        )}
        <div className={styles.lightboxMeta}>
          <span className={styles.lightboxCaption}>{label}</span>
          {total > 1 ? (
            <span className={styles.lightboxCounter}>
              {t('videoGeneration.agentSession.preview.counter', {
                current: index + 1,
                total,
                defaultValue: '{{current}} / {{total}}',
              })}
            </span>
          ) : null}
        </div>
      </div>
    </div>,
    document.body
  );
};

export default StudioMediaLightbox;
