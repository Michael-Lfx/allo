

import { Left, Right } from '@icon-park/react';
import React, { Fragment, useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
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
const SHEET_TRANSITION_MS = 280;
const REDUCED_MOTION_QUERY = '(prefers-reduced-motion: reduce)';

const prefersReducedMotion = (): boolean =>
  typeof window !== 'undefined' && window.matchMedia?.(REDUCED_MOTION_QUERY).matches === true;

const getFocusableElements = (container: HTMLElement): HTMLElement[] =>
  Array.from(
    container.querySelectorAll<HTMLElement>(
      'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
    ),
  ).filter((element) => {
    if (element.hidden || element.getAttribute('aria-disabled') === 'true') return false;
    if (element.closest('[aria-hidden="true"]')) return false;
    return element.getClientRects().length > 0;
  });

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
  const focusTimeoutRef = useRef<number | null>(null);
  const sheetRef = useRef<HTMLDivElement>(null);
  const previouslyFocusedRef = useRef<HTMLElement | null>(null);
  const sheetTitleId = `mobile-action-sheet-title-${useId().replace(/:/g, '')}`;

  const restoreFocus = useCallback(() => {
    const previous = previouslyFocusedRef.current;
    const fallback = document.querySelector<HTMLElement>(
      '[data-testid="nomi-sendbox"] textarea, [data-testid="guid-send-btn"], [data-testid="nomi-sendbox"] button',
    );
    const target = previous?.isConnected ? previous : fallback;
    if (target && target.isConnected && !target.hasAttribute('disabled')) {
      target.focus({ preventScroll: true });
    }
    previouslyFocusedRef.current = null;
  }, []);

  const setSheetElement = useCallback(
    (element: HTMLDivElement | null) => {
      sheetRef.current = element;
      notificationBlockerRef(element);
    },
    [notificationBlockerRef],
  );

  useEffect(() => () => restoreFocus(), [restoreFocus]);

  // Mount / unmount lifecycle — drives DOM presence only.
  useEffect(() => {
    if (open) {
      if (!previouslyFocusedRef.current) {
        const activeElement = document.activeElement;
        if (activeElement instanceof HTMLElement) previouslyFocusedRef.current = activeElement;
      }
      setMounted(true);
      return;
    }
    setVisible(false);
    setActiveSubKey(null);
    const reducedMotion = prefersReducedMotion();
    const t = setTimeout(() => {
      setMounted(false);
      restoreFocus();
    }, reducedMotion ? 0 : SHEET_TRANSITION_MS);
    return () => clearTimeout(t);
  }, [open, restoreFocus]);

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
      }, prefersReducedMotion() ? 0 : TRANSITION_MS);
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

  useLayoutEffect(() => {
    if (!open || !mounted || !visible || (activeSubKey && subPhase !== 'shown')) return undefined;
    const focusFirstAvailable = () => {
      const sheet = sheetRef.current;
      const firstFocusable = sheet ? getFocusableElements(sheet)[0] : undefined;
      (firstFocusable ?? sheet)?.focus({ preventScroll: true });
    };
    const frame = requestAnimationFrame(() => {
      const timeout = window.setTimeout(
        () => {
          focusTimeoutRef.current = null;
          focusFirstAvailable();
        },
        prefersReducedMotion() ? 0 : activeSubKey ? TRANSITION_MS : SHEET_TRANSITION_MS,
      );
      focusTimeoutRef.current = timeout;
    });
    return () => {
      cancelAnimationFrame(frame);
      if (focusTimeoutRef.current !== null) {
        window.clearTimeout(focusTimeoutRef.current);
        focusTimeoutRef.current = null;
      }
    };
  }, [activeSubKey, mounted, open, subPhase, visible]);

  useEffect(() => {
    if (!mounted || !open) return undefined;

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        if (activeSubKey) {
          setActiveSubKey(null);
        } else {
          onClose();
        }
        return;
      }
      if (event.key !== 'Tab') return;

      const sheet = sheetRef.current;
      if (!sheet) return;
      const focusableElements = getFocusableElements(sheet);
      if (focusableElements.length === 0) {
        event.preventDefault();
        sheet.focus({ preventScroll: true });
        return;
      }

      const currentIndex = focusableElements.indexOf(document.activeElement as HTMLElement);
      const nextIndex = event.shiftKey
        ? currentIndex <= 0
          ? focusableElements.length - 1
          : currentIndex - 1
        : currentIndex === focusableElements.length - 1
          ? 0
          : currentIndex + 1;
      event.preventDefault();
      focusableElements[nextIndex]?.focus({ preventScroll: true });
    };

    document.addEventListener('keydown', handleKeyDown, true);
    return () => document.removeEventListener('keydown', handleKeyDown, true);
  }, [activeSubKey, mounted, onClose, open]);

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
        ref={setSheetElement}
        className={`${styles.sheet} ${visible ? styles.visible : ''}`}
        role='dialog'
        aria-modal='true'
        aria-labelledby={title ? sheetTitleId : undefined}
        aria-label={title ? undefined : t('common.more', { defaultValue: 'More' })}
        tabIndex={-1}
        onClick={(e) => e.stopPropagation()}
      >
        <div className={styles.handle} />
        <div className={styles.panes}>
          <div
            className={`${styles.pane} ${styles.paneMain} ${subPhase === 'shown' ? styles.paneOutLeft : styles.paneActive}`}
            aria-hidden={subPhase === 'shown'}
          >
            {title && (
              <div id={sheetTitleId} className={styles.header}>
                {title}
              </div>
            )}
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
