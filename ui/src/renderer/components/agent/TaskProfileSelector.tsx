/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import classNames from 'classnames';
import { Briefcase, Code } from '@icon-park/react';
import React, { useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { useTaskProfileStore, normalizeTaskProfile, type TaskProfile } from '@/renderer/stores/taskProfileStore';
import styles from './TaskProfileSelector.module.css';

export type { TaskProfile } from '@/renderer/stores/taskProfileStore';

export interface TaskProfileSelectorProps {
  /** Fired after a local selection (optional local listeners). */
  onProfileSelect?: (profile: TaskProfile) => void;
  disabled?: boolean;
  className?: string;
}

const PROFILES: readonly TaskProfile[] = ['office', 'coding'] as const;

function profileIcon(value: TaskProfile) {
  switch (value) {
    case 'office':
      return <Briefcase theme='outline' size={16} fill='currentColor' />;
    case 'coding':
      return <Code theme='outline' size={16} fill='currentColor' />;
    default: {
      const _exhaustive: never = value;
      return _exhaustive;
    }
  }
}

/**
 * Segmented work-mode control for choosing the Nomi profile. The selection is
 * app-global and persisted (see {@link useTaskProfileStore}) so it survives
 * navigation between modules and app restarts; a conversation freezes its own
 * value at creation.
 */
const TaskProfileSelector: React.FC<TaskProfileSelectorProps> = ({ onProfileSelect, disabled = false, className }) => {
  const { t } = useTranslation();
  const profile = useTaskProfileStore((state) => state.taskProfile);
  const setTaskProfile = useTaskProfileStore((state) => state.setTaskProfile);

  const label = t('conversation.taskProfile.label', { defaultValue: '工作模式' });
  const officeLabel = t('conversation.taskProfile.office', { defaultValue: '日常办公' });
  const codingLabel = t('conversation.taskProfile.coding', { defaultValue: '代码开发' });

  const profileLabel = (value: TaskProfile) => (value === 'coding' ? codingLabel : officeLabel);

  const handleSelect = useCallback(
    (next: TaskProfile) => {
      const normalized = normalizeTaskProfile(next);
      if (disabled || normalized === profile) return;
      setTaskProfile(normalized);
      onProfileSelect?.(normalized);
    },
    [disabled, onProfileSelect, profile, setTaskProfile]
  );

  return (
    <div
      className={classNames(styles.track, className)}
      role='radiogroup'
      aria-label={label}
      data-testid='task-profile-selector'
    >
      {PROFILES.map((value) => {
        const active = value === profile;
        return (
          <button
            key={value}
            type='button'
            role='radio'
            aria-checked={active}
            data-button-shape='pill'
            data-testid={`task-profile-option-${value}`}
            className={classNames(styles.option, active && styles.optionActive)}
            disabled={disabled}
            onClick={() => handleSelect(value)}
          >
            <span className={styles.optionIcon} aria-hidden='true'>
              {profileIcon(value)}
            </span>
            <span className={styles.optionLabel}>{profileLabel(value)}</span>
          </button>
        );
      })}
    </div>
  );
};

export default TaskProfileSelector;
