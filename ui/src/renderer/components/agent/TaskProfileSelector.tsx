/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import classNames from 'classnames';
import { Briefcase, Code } from '@icon-park/react';
import React, { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './TaskProfileSelector.module.css';

export type TaskProfile = 'office' | 'coding';

export interface TaskProfileSelectorProps {
  /** Current / preferred profile. Defaults to office. */
  initialProfile?: TaskProfile;
  /** Fired after a local selection (Guid pre-create only). */
  onProfileSelect?: (profile: TaskProfile) => void;
  disabled?: boolean;
  className?: string;
}

const PROFILES: readonly TaskProfile[] = ['office', 'coding'] as const;

function normalizeProfile(value: string | undefined): TaskProfile {
  return value === 'coding' ? 'coding' : 'office';
}

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
 * Segmented work-mode control for choosing Nomi profile before a conversation
 * starts. Mid-session switching is intentionally unsupported — profile is
 * fixed at create.
 */
const TaskProfileSelector: React.FC<TaskProfileSelectorProps> = ({
  initialProfile = 'office',
  onProfileSelect,
  disabled = false,
  className,
}) => {
  const { t } = useTranslation();
  const [profile, setProfile] = useState<TaskProfile>(() => normalizeProfile(initialProfile));

  const label = t('conversation.taskProfile.label', { defaultValue: '工作模式' });
  const officeLabel = t('conversation.taskProfile.office', { defaultValue: '日常办公' });
  const codingLabel = t('conversation.taskProfile.coding', { defaultValue: '代码开发' });

  const profileLabel = (value: TaskProfile) =>
    value === 'coding' ? codingLabel : officeLabel;

  useEffect(() => {
    setProfile(normalizeProfile(initialProfile));
  }, [initialProfile]);

  const handleSelect = useCallback(
    (next: TaskProfile) => {
      if (disabled || next === profile) return;
      setProfile(next);
      onProfileSelect?.(next);
    },
    [disabled, onProfileSelect, profile]
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
