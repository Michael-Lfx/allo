import React, { useEffect, useRef } from 'react';
import { Close, Left } from '@icon-park/react';
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
}) => {
  const firstOptionRef = useRef<HTMLButtonElement>(null);
  const hasMedia = options.some((option) => option.cover || option.preview || option.tree);

  useEffect(() => {
    const first = options.find((option) => !option.disabled);
    if (first) {
      const frame = window.requestAnimationFrame(() => firstOptionRef.current?.focus());
      return () => window.cancelAnimationFrame(frame);
    }
    return undefined;
  }, [options]);

  return (
    <section className={`${styles.drawer} ${hasMedia ? styles.drawerMedia : ''}`} data-testid={testId}>
      <header className={styles.header}>
        {onBack ? (
          <button type='button' className={styles.back} onClick={onBack}>
            <Left theme='outline' size={14} />
            <span>{backLabel}</span>
          </button>
        ) : (
          <h2 className={styles.title}>{title}</h2>
        )}
        <button type='button' className={styles.close} onClick={onClose} aria-label={closeLabel}>
          <Close theme='outline' size={16} />
        </button>
      </header>
      <div className={hasMedia ? styles.mediaGrid : styles.options}>
        {options.map((option, index) => {
          const isFirstEnabled = index === options.findIndex((item) => !item.disabled);
          return (
            <button
              key={option.key}
              ref={isFirstEnabled ? firstOptionRef : undefined}
              type='button'
              className={`${styles.option} ${hasMedia ? styles.mediaOption : ''} ${
                option.emphasis ? styles.mediaOptionEmphasis : ''
              }`}
              data-testid={`guid-home-option-${option.key}`}
              disabled={option.disabled}
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
                {option.meta ? <span className={styles.optionMeta}>{option.meta}</span> : null}
              </span>
            </button>
          );
        })}
      </div>
    </section>
  );
};

export default HomeInputDrawer;
