import { CloseSmall, Puzzle, Robot } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import styles from '../index.module.css';

export interface ComposerEntryStripProps {
  isPresetAgent: boolean;
  presetLabel?: string;
  presetAvatar?: { kind: 'image' | 'emoji' | 'icon'; value?: string };
  onChoosePreset: () => void;
  onFree: () => void;
  /** Opens the shared Skills selector for the current draft. */
  onAdjustSkills?: () => void;
  activeSkillCount?: number;
  /** Conversation composers only need the Skills control. */
  hidePreset?: boolean;
  skillsButtonTestId?: string;
  forceOpaque?: boolean;
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
  onAdjustSkills,
  activeSkillCount = 0,
  hidePreset = false,
  skillsButtonTestId = 'guid-adjust-skills',
  forceOpaque = false,
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

  const skillButton = onAdjustSkills ? (
    <button
      type='button'
      data-testid={skillsButtonTestId}
      className={`${styles.entryButton} ${styles.entryButtonInteractive} ${activeSkillCount > 0 ? styles.entryButtonActive : ''}`}
      onClick={onAdjustSkills}
      aria-label={t('guid.entry.adjustSkills', { defaultValue: 'Adjust Skills' })}
      title={t('guid.entry.adjustSkills', { defaultValue: 'Adjust Skills' })}
    >
      <Puzzle theme='outline' size={15} fill='currentColor' />
      <span className={styles.entryButtonText}>{t('guid.entry.skills', { defaultValue: 'Skills' })}</span>
      {activeSkillCount > 0 && (
        <span className={styles.entryCountBadge} aria-label={t('guid.entry.skillCount', { count: activeSkillCount })}>
          <span className={styles.entryCountBadgeDigit}>{activeSkillCount}</span>
        </span>
      )}
    </button>
  ) : null;

  if (hidePreset) {
    if (!skillButton) return null;
    return (
      <div className={styles.entryStrip} style={forceOpaque ? { opacity: 1 } : undefined}>
        {skillButton}
      </div>
    );
  }

  if (isPresetAgent) {
    return (
      <div className={styles.entryStrip}>
        <span
          className={`${styles.entryButton} ${styles.entryButtonActive} ${styles.entryPersonaButton}`}
          title={presetLabel || t('guid.entry.usePreset', { defaultValue: 'Use preset' })}
        >
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
        {skillButton}
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
        aria-label={t('guid.entry.usePreset', { defaultValue: 'Use preset' })}
        title={t('guid.entry.usePreset', { defaultValue: 'Use preset' })}
      >
        <Robot theme='outline' size={15} fill='currentColor' />
        <span className={styles.entryButtonText}>{t('guid.entry.usePreset', { defaultValue: 'Use preset' })}</span>
      </button>
      {skillButton}
    </div>
  );
};

export default ComposerEntryStrip;
