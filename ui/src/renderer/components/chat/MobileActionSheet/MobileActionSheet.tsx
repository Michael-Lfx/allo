

import { Left, Right } from '@icon-park/react';
import React, { Fragment, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import { useNotificationBlocker } from '@/renderer/components/notifications';
import styles from './MobileActionSheet.module.css';
import type {
  MobileActionSheetEntry,
  MobileActionSheetOption,
  MobileActionSheetProps,
  MobileActionSheetSubMenu,
} from './types';

const TRANSITION_MS = 260;

const MobileActionSheet: React.FC<MobileActionSheetProps> = ({ open, onClose, title, entries }) => {
  const { t } = useTranslation();
  const [activeSubKey, setActiveSubKey] = useState<string | null>(null);
  // Sub pane stays mounted briefly after deactivation so its slide-out animation
  // can play. `subPhase` drives the animation: 'enter' positions the sub pane
  // off-screen (right) before the next frame flips to 'shown', so the CSS
  // transition has a starting point.
  const [renderedSubKey, setRenderedSubKey] = useState<string | null>(null);
  const [subPhase, setSubPhase] = useState<'idle' | 'enter' | 'shown' | 'exit'>('idle');
  const [mounted, setMounted] = useState(false);
  const notificationBlockerRef = useNotificationBlocker(mounted);
  // `visible` lags `mounted` by one paint so the sheet renders at
  // translateY(100%) first, then the next frame transitions to translateY(0).
  // Without this gap, applying .visible on first mount skips the slide-up
  // (perceived as a flash). Crucially we run the visibility flip in a
  // *separate* layout effect — coupling it to `mounted` (instead of `open`)
  // forces React to commit the off-screen frame before the rAF kicks in.
  const [visible, setVisible] = useState(false);
  const openRafRef = useRef<number | null>(null);

  // Mount / unmount lifecycle — drives DOM presence only.
  useEffect(() => {
    if (open) {
      setMounted(true);
      return;
    }
    setVisible(false);
    setActiveSubKey(null);
    const t = setTimeout(() => setMounted(false), 280);
    return () => clearTimeout(t);
  }, [open]);

  // Visibility lifecycle — flips `.visible` only after the off-screen frame
  // has been painted. Using useLayoutEffect with a `mounted` dependency
  // guarantees we observe the freshly committed DOM before scheduling the rAF;
  // this avoids React 18 batching collapsing mount + visible into one paint
  // (which produced the inconsistent "snap up" animation).
  useLayoutEffect(() => {
    if (!open || !mounted) return;
    const raf1 = requestAnimationFrame(() => {
      const raf2 = requestAnimationFrame(() => setVisible(true));
      openRafRef.current = raf2;
    });
    openRafRef.current = raf1;
    return () => {
      if (openRafRef.current !== null) cancelAnimationFrame(openRafRef.current);
    };
  }, [open, mounted]);

  useEffect(() => {
    if (activeSubKey) {
      setRenderedSubKey(activeSubKey);
      setSubPhase('enter');
      const raf = requestAnimationFrame(() => {
        requestAnimationFrame(() => setSubPhase('shown'));
      });
      return () => cancelAnimationFrame(raf);
    }
    if (renderedSubKey) {
      setSubPhase('exit');
      const id = setTimeout(() => {
        setRenderedSubKey(null);
        setSubPhase('idle');
      }, TRANSITION_MS);
      return () => clearTimeout(id);
    }
  }, [activeSubKey, renderedSubKey]);

  useEffect(() => {
    if (!open) return;
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    return () => {
      document.body.style.overflow = previousOverflow;
    };
  }, [open]);

  const activeEntry = activeSubKey ? entries.find((e) => e.key === activeSubKey) : null;
  const activeSub: MobileActionSheetSubMenu | undefined = activeEntry?.submenu;
  const renderedSubEntry = renderedSubKey ? entries.find((e) => e.key === renderedSubKey) : null;
  const renderedSub: MobileActionSheetSubMenu | undefined = renderedSubEntry?.submenu;

  if (!mounted) {
    return null;
  }

  const handleEntryClick = (entry: MobileActionSheetEntry) => {
    if (entry.disabled) return;
    if (entry.submenu) {
      setActiveSubKey(entry.key);
      return;
    }
    entry.onClick?.();
    onClose();
  };

  const handleSubSelect = (key: string) => {
    if (!activeSub) return;
    activeSub.onSelect(key);
    // For settings (model, permission) the user expects to see the new value
    // reflected on the main pane, so we slide back instead of dismissing the
    // sheet. For non-selectable submenus (skills, attach) the selection is
    // an action — close the sheet so the user can immediately interact with
    // the result (e.g. type a slash command, see attached files).
    if (activeSub.selectable !== false) {
      setActiveSubKey(null);
      return;
    }
    onClose();
  };

  const activateOnKeyboard = (event: React.KeyboardEvent, action: () => void) => {
    if (event.key !== 'Enter' && event.key !== ' ') return;
    event.preventDefault();
    action();
  };

  const renderSubmenuOption = (option: MobileActionSheetOption) => {
    const showRadio = renderedSub?.selectable !== false;
    return (
      <div
        key={option.key}
        className={`${styles.item} ${option.disabled ? styles.disabled : ''}`}
        role='button'
        tabIndex={subPhase === 'shown' && !option.disabled ? 0 : -1}
        aria-disabled={option.disabled || undefined}
        aria-pressed={showRadio ? option.active || false : undefined}
        onClick={() => {
          if (option.disabled) return;
          handleSubSelect(option.key);
        }}
        onKeyDown={(event) => {
          if (option.disabled) return;
          activateOnKeyboard(event, () => handleSubSelect(option.key));
        }}
        data-testid={`mobile-action-sheet-option-${option.key}`}
      >
        <div className={styles.body}>
          <div className={styles.label}>{option.label}</div>
          {option.description && <div className={styles.desc}>{option.description}</div>}
        </div>
        {showRadio && (
          <div className={`${styles.radio} ${option.active ? styles.checked : ''}`} aria-hidden='true' />
        )}
      </div>
    );
  };

  const submenuOptions = renderedSub?.options ?? [];
  const submenuGroups = renderedSub?.groups?.filter((group) => group.options.length > 0) ?? [];
  const hasSubmenuOptions = submenuOptions.length > 0 || submenuGroups.length > 0;

  return createPortal(
    <Fragment>
      <div className={`${styles.mask} ${visible ? styles.visible : ''}`} onClick={onClose} />
      <div
        ref={notificationBlockerRef}
        className={`${styles.sheet} ${visible ? styles.visible : ''}`}
        role='dialog'
        aria-modal='true'
        onClick={(e) => e.stopPropagation()}
      >
        <div className={styles.handle} />
        <div className={styles.panes}>
          <div
            className={`${styles.pane} ${styles.paneMain} ${subPhase === 'shown' ? styles.paneOutLeft : styles.paneActive}`}
            aria-hidden={subPhase === 'shown'}
          >
            {title && <div className={styles.header}>{title}</div>}
            <div className={styles.list}>
              {entries.map((entry, index) => (
                <Fragment key={entry.key}>
                  {entry.dividerBefore && index !== 0 && <div className={styles.divider} />}
                  <div
                    className={`${styles.item} ${entry.disabled ? styles.disabled : ''}`}
                    role='button'
                    tabIndex={subPhase === 'shown' || entry.disabled ? -1 : 0}
                    aria-disabled={entry.disabled || undefined}
                    aria-haspopup={entry.submenu ? 'dialog' : undefined}
                    aria-expanded={entry.submenu ? activeSubKey === entry.key : undefined}
                    onClick={() => handleEntryClick(entry)}
                    onKeyDown={(event) => {
                      if (entry.disabled) return;
                      activateOnKeyboard(event, () => handleEntryClick(entry));
                    }}
                    data-testid={`mobile-action-sheet-${entry.key}`}
                  >
                    {entry.icon && (
                      <div className={`${styles.icon} ${entry.variant === 'muted' ? styles.muted : ''}`}>
                        {entry.icon}
                      </div>
                    )}
                    <div className={styles.body}>
                      <div className={styles.label}>{entry.label}</div>
                      {entry.description && <div className={styles.desc}>{entry.description}</div>}
                    </div>
                    {(entry.meta || entry.submenu) && (
                      <div className={styles.meta}>
                        {entry.meta && <span className={styles.metaText}>{entry.meta}</span>}
                        {entry.submenu && (
                          <Right theme='outline' size='14' className={styles.chevron} aria-hidden='true' />
                        )}
                      </div>
                    )}
                  </div>
                </Fragment>
              ))}
            </div>
          </div>

          {renderedSub && (
            <div
              className={`${styles.pane} ${styles.paneSub} ${subPhase === 'shown' ? styles.paneActive : styles.paneOutRight}`}
              aria-hidden={subPhase !== 'shown'}
            >
              <div className={styles.subbar}>
                <button
                  className={styles.back}
                  onClick={() => setActiveSubKey(null)}
                  tabIndex={subPhase === 'shown' ? 0 : -1}
                  type='button'
                >
                  <Left theme='outline' size='16' />
                  <span>{t('common.back', { defaultValue: 'Back' })}</span>
                </button>
                <div className={styles.subtitle}>{renderedSub.title}</div>
              </div>
              <div className={styles.list}>
                {!hasSubmenuOptions ? (
                  <div className={styles.empty}>{renderedSub.emptyText}</div>
                ) : (
                  <>
                    {submenuOptions.map(renderSubmenuOption)}
                    {submenuGroups.map((group) => (
                      <Fragment key={group.key}>
                        <div className={styles.groupHeader} role='heading' aria-level={3}>
                          {group.title}
                        </div>
                        {group.options.map(renderSubmenuOption)}
                      </Fragment>
                    ))}
                  </>
                )}
              </div>
            </div>
          )}
        </div>
      </div>
    </Fragment>,
    document.body
  );
};

export default MobileActionSheet;
