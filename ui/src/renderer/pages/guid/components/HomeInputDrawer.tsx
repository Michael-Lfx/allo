import React, { useCallback, useEffect, useRef, useState } from 'react';
import { Close, Left, Right } from '@icon-park/react';
import { useTranslation } from 'react-i18next';
import type { HomeStageOption } from './homeStage';
import styles from './HomeInputDrawer.module.css';

type HomeInputDrawerProps = {
  /** Layer title, e.g. the build/create group name. */
  title: string;
  options: HomeStageOption[];
  onSelect: (option: HomeStageOption) => void;
  onClose: () => void;
  /** Optional back affordance for sub layers. Top-level drawers only close. */
  onBack?: () => void;
  backLabel?: string;
  closeLabel?: string;
  testId?: string;
  /** Build-level transition: exit collapses into the composer, expand grows from it. */
  motion?: 'enter' | 'expand' | 'exit' | 'idle';
  /** One source of truth for the CSS animation timing in milliseconds. */
  motionDuration?: number;
  /** Fired when the drawer's own CSS transition finishes. */
  onMotionEnd?: (motion: 'enter' | 'expand' | 'exit' | 'idle') => void;
  /** Render media cards in a fixed-width horizontal rail with arrow controls. */
  scrollableMedia?: boolean;
};

/** Low-contrast project tree outline for the "open existing project" item. */
function ProjectTreeOutline() {
  return (
    <svg className={styles.tree} viewBox='0 0 120 68' aria-hidden='true' focusable='false'>
      <path d='M14 16h20l6 7h22' className={styles.treeLine} />
      <path d='M14 32h24l6 7h28' className={styles.treeLine} />
      <path d='M14 48h28l6 7h30' className={styles.treeLine} />
      <rect className={styles.treeFolder} x='10' y='10' width='22' height='9' rx='2' />
      <rect className={styles.treeLine} x='58' y='14' width='38' height='7' rx='2' />
      <rect className={styles.treeLine} x='58' y='30' width='30' height='7' rx='2' />
      <rect className={styles.treeLine} x='58' y='46' width='34' height='7' rx='2' />
    </svg>
  );
}

/**
 * Directory drawer pinned above the composer: same width as the composer,
 * 8px gap, 32px header. Options can carry a real cover/preview (what the user
 * will get) or stay as compact text entries. It only changes the visible layer;
 * drafts and the running capability are untouched.
 */
const HomeInputDrawer: React.FC<HomeInputDrawerProps> = ({
  title,
  options,
  onSelect,
  onClose,
  onBack,
  backLabel,
  closeLabel,
  testId,
  motion = 'enter',
  motionDuration,
  onMotionEnd,
  scrollableMedia = false,
}) => {
  const { t } = useTranslation();
  const firstOptionRef = useRef<HTMLButtonElement>(null);
  const drawerRef = useRef<HTMLElement>(null);
  const mediaRailRef = useRef<HTMLDivElement>(null);
  const [mediaEdges, setMediaEdges] = useState({ start: true, end: false });
  const hasMedia = options.some((option) => option.cover || option.preview || option.tree);

  // Focus the first option when a new drawer layer mounts, not when a
  // selected card re-renders the same option list (that would fight the
  // composer focus after picking an example).
  useEffect(() => {
    const first = options.find((option) => !option.disabled);
    if (first) {
      const frame = window.requestAnimationFrame(() => firstOptionRef.current?.focus());
      return () => window.cancelAnimationFrame(frame);
    }
    return undefined;
  }, [options.length, testId]);

  const updateMediaEdges = useCallback(() => {
    const rail = mediaRailRef.current;
    if (!rail) return;
    const start = rail.scrollLeft <= 1;
    const end = rail.scrollLeft + rail.clientWidth >= rail.scrollWidth - 1;
    setMediaEdges((current) => (current.start === start && current.end === end ? current : { start, end }));
  }, []);

  useEffect(() => {
    const rail = mediaRailRef.current;
    if (!rail || !scrollableMedia) return;
    updateMediaEdges();
    const observer = new ResizeObserver(updateMediaEdges);
    observer.observe(rail);
    return () => observer.disconnect();
  }, [scrollableMedia, updateMediaEdges]);

  const scrollMedia = (direction: -1 | 1) => {
    const rail = mediaRailRef.current;
    if (!rail) return;
    rail.scrollBy({ left: direction * rail.clientWidth * 0.9, behavior: 'smooth' });
  };

  useEffect(() => {
    const drawer = drawerRef.current;
    if (!drawer) return undefined;
    if (motion === 'exit') drawer.setAttribute('inert', '');
    else drawer.removeAttribute('inert');
    return () => drawer.removeAttribute('inert');
  }, [motion]);

  return (
    <section
      ref={drawerRef}
      className={`${styles.drawer} ${hasMedia ? styles.drawerMedia : ''}`}
      data-testid={testId}
      data-motion={motion}
      aria-hidden={motion === 'exit' ? true : undefined}
      onAnimationEnd={(event) => {
        if (event.target === event.currentTarget) onMotionEnd?.(motion);
      }}
      style={
        motionDuration
          ? ({ '--drawer-motion-duration': `${motionDuration}ms` } as React.CSSProperties)
          : undefined
      }
    >
      <header className={styles.header}>
        {onBack ? (
          <button type='button' className={styles.back} onClick={onBack}>
            <Left theme='outline' size={14} />
            <span>{backLabel}</span>
          </button>
        ) : (
          <h2 className={styles.title}>{title}</h2>
        )}
        <span className={styles.headerActions}>
          {scrollableMedia ? (
            <>
              <button
                type='button'
                className={styles.iconButton}
                aria-label={t('presentation.create.previous')}
                disabled={mediaEdges.start}
                onClick={() => scrollMedia(-1)}
              >
                <Left theme='outline' size={16} fill='currentColor' />
              </button>
              <button
                type='button'
                className={styles.iconButton}
                aria-label={t('presentation.create.next')}
                disabled={mediaEdges.end}
                onClick={() => scrollMedia(1)}
              >
                <Right theme='outline' size={16} fill='currentColor' />
              </button>
            </>
          ) : null}
          <button type='button' className={styles.close} onClick={onClose} aria-label={closeLabel}>
            <Close theme='outline' size={16} />
          </button>
        </span>
      </header>
      <div
        ref={scrollableMedia ? mediaRailRef : undefined}
        className={
          hasMedia
            ? `${styles.mediaGrid} ${scrollableMedia ? styles.mediaGridScrollable : ''}`
            : styles.options
        }
        onScroll={scrollableMedia ? updateMediaEdges : undefined}
      >
        {options.map((option, index) => {
          const isFirstEnabled = index === options.findIndex((item) => !item.disabled);
          return (
            <button
              key={option.key}
              ref={isFirstEnabled ? firstOptionRef : undefined}
              type='button'
              className={`${styles.option} ${hasMedia ? styles.mediaOption : ''} ${
                option.emphasis ? styles.mediaOptionEmphasis : ''
              } ${option.selected ? styles.mediaOptionSelected : ''}`}
              data-testid={`guid-home-option-${option.key}`}
              disabled={option.disabled}
              aria-pressed={option.selected || undefined}
              onClick={() => onSelect(option)}
            >
              {hasMedia ? (
                <span className={styles.thumb}>
                  {option.cover ? (
                    <img src={option.cover} alt={option.coverAlt ?? ''} className={styles.thumbImage} />
                  ) : option.preview ? (
                    option.preview
                  ) : option.tree ? (
                    <ProjectTreeOutline />
                  ) : null}
                </span>
              ) : null}
              <span className={styles.optionCopy}>
                <span className={styles.optionTitle}>{option.title}</span>
                {option.description ? <span className={styles.optionDescription}>{option.description}</span> : null}
              </span>
            </button>
          );
        })}
      </div>
    </section>
  );
};

export default HomeInputDrawer;
