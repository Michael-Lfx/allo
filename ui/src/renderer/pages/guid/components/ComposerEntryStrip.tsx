import { CloseSmall, Puzzle, Robot } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import TaskProfileSelector, {
  type TaskProfile,
} from '@/renderer/components/agent/TaskProfileSelector';
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
  /** Nomi session work mode (office | coding). */
  taskProfile?: TaskProfile;
  onTaskProfileChange?: (profile: TaskProfile) => void;
  /** Hide the work-mode toggle (e.g. non-Nomi engines). */
  hideTaskProfile?: boolean;
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
  taskProfile = 'office',
  onTaskProfileChange,
  hideTaskProfile = false,
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

  const taskProfileSelector = hideTaskProfile ? null : (
    <TaskProfileSelector
      initialProfile={taskProfile}
      onProfileSelect={onTaskProfileChange}
    />
  );

  const skillButton = onAdjustSkills ? (
    <button
      type='button'
      data-testid='guid-adjust-skills'
      className={`${styles.entryButton} ${styles.entryButtonInteractive} ${activeSkillCount > 0 ? styles.entryButtonActive : ''}`}
      onClick={onAdjustSkills}
      aria-label={t('guid.entry.adjustSkills', { defaultValue: 'Adjust Skills' })}
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

  if (isPresetAgent) {
    return (
      <div className={styles.entryStrip}>
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
        {skillButton}
        {taskProfileSelector}
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
      {skillButton}
      {taskProfileSelector}
    </div>
  );
};

export default ComposerEntryStrip;
