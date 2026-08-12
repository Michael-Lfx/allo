import { ApplicationOne, CloseSmall, Robot } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import styles from '../index.module.css';

export interface ComposerEntryStripProps {
  isPresetAgent: boolean;
  presetLabel?: string;
  presetAvatar?: { kind: 'image' | 'emoji' | 'icon'; value?: string };
  onChoosePreset: () => void;
  onFree: () => void;
  /** 「创建小程序」entry. Omit to hide the capability on a surface entirely. */
  onCreateMiniApp?: () => void;
  /** True while the composer is in mini-app mode (shows a dismissible token). */
  miniAppActive?: boolean;
  /** Leaves mini-app mode. */
  onDismissMiniApp?: () => void;
}

/**
 * Entry controls that describe how a new conversation is created. Explicit
 * Skill loads intentionally live in the composer body so they remain per-turn
 * choices instead of looking like a persistent conversation setting.
 */
const ComposerEntryStrip: React.FC<ComposerEntryStripProps> = ({
  isPresetAgent,
  presetLabel,
  presetAvatar,
  onChoosePreset,
  onFree,
  onCreateMiniApp,
  miniAppActive = false,
  onDismissMiniApp,
}) => {
  const { t } = useTranslation();

  const renderAvatar = () => {
    if (!presetAvatar) return <Robot theme='outline' size={16} fill='currentColor' />;
    switch (presetAvatar.kind) {
      case 'image':
        return <img src={presetAvatar.value} alt='' className='w-20px h-20px rounded-6px object-contain' />;
      case 'emoji':
        return <span className='text-14px leading-none'>{presetAvatar.value}</span>;
      case 'icon':
      default:
        return <Robot theme='outline' size={16} fill='currentColor' />;
    }
  };

  // Shown for every engine selection: mini-app mode pins the launch to the Nomi
  // engine itself (spec D4), so it overrides the pill rather than depending on it.
  // Active state copies the preset persona token: an accented, dismissible chip.
  const miniAppEntry = miniAppActive ? (
    <span
      className={`${styles.entryButton} ${styles.entryButtonActive} ${styles.entryPersonaButton}`}
      data-testid='guid-miniapp-token'
    >
      <span className={styles.entryAvatar}>
        <ApplicationOne theme='outline' size={16} fill='currentColor' />
      </span>
      <span className={styles.entryButtonText}>{t('miniApps.composer.activeLabel')}</span>
      <button
        type='button'
        className={styles.entryDismiss}
        onClick={onDismissMiniApp}
        aria-label={t('miniApps.composer.dismiss')}
      >
        <CloseSmall theme='outline' size={14} />
      </button>
    </span>
  ) : onCreateMiniApp ? (
    <button
      type='button'
      data-button-shape='pill'
      className={`${styles.entryButton} ${styles.entryButtonInteractive}`}
      onClick={onCreateMiniApp}
      aria-label={t('miniApps.composer.entry')}
      data-testid='guid-miniapp-entry'
    >
      <ApplicationOne theme='outline' size={15} fill='currentColor' />
      <span className={styles.entryButtonText}>{t('miniApps.composer.entry')}</span>
    </button>
  ) : null;

  if (isPresetAgent) {
    return (
      <div className={styles.entryStrip}>
        {miniAppEntry}
        <span className={`${styles.entryButton} ${styles.entryButtonActive} ${styles.entryPersonaButton}`}>
          <span className={styles.entryAvatar}>{renderAvatar()}</span>
          <span className={styles.entryButtonText}>
            {presetLabel || t('guid.entry.usePreset', { defaultValue: 'Use preset' })}
          </span>
        </span>
        <button
          type='button'
          className={styles.entryDismiss}
          onClick={onFree}
          aria-label={t('guid.entry.backToFree', { defaultValue: 'Freeform' })}
          title={t('guid.entry.backToFree', { defaultValue: 'Freeform' })}
        >
          <CloseSmall theme='outline' size={14} />
        </button>
      </div>
    );
  }

  return (
    <div className={styles.entryStrip}>
      <button
        type='button'
        data-button-shape='pill'
        className={`${styles.entryButton} ${styles.entryButtonInteractive}`}
        onClick={onChoosePreset}
      >
        <Robot theme='outline' size={15} fill='currentColor' />
        <span className={styles.entryButtonText}>{t('guid.entry.usePreset', { defaultValue: 'Use preset' })}</span>
      </button>
      {miniAppEntry}
    </div>
  );
};

export default ComposerEntryStrip;
