/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Briefcase, Code } from '@icon-park/react';
import React, { useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import GooeyNav from '@renderer/components/base/GooeyNav';
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
 * Work-mode control for choosing the Nomi profile. The selection is app-global
 * and persisted (see {@link useTaskProfileStore}) so it survives navigation
 * between modules and app restarts; a conversation freezes its own value at
 * creation.
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
    <GooeyNav
      className={className}
      testId='task-profile-selector'
      ariaLabel={label}
      selection='radio'
      size='md'
      disabled={disabled}
      activeIndex={Math.max(0, PROFILES.indexOf(profile))}
      items={PROFILES.map((value) => ({
        label: profileLabel(value),
        icon: <span className={styles.optionIcon}>{profileIcon(value)}</span>,
        testId: `task-profile-option-${value}`,
      }))}
      onChange={(index) => handleSelect(PROFILES[index])}
    />
  );
};

export default TaskProfileSelector;
