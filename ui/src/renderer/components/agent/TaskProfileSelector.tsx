/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Briefcase, Code } from '@icon-park/react';
import { Dropdown, Menu } from '@arco-design/web-react';
import React, { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';
import dropdownMenuStyles from '@/renderer/styles/configDropdownMenu.module.css';
import { useTaskProfileStore, normalizeTaskProfile, type TaskProfile } from '@/renderer/stores/taskProfileStore';
import styles from './TaskProfileSelector.module.css';

export type { TaskProfile } from '@/renderer/stores/taskProfileStore';

export interface TaskProfileSelectorProps {
  /** Fired after a local selection (optional local listeners). */
  onProfileSelect?: (profile: TaskProfile) => void;
  disabled?: boolean;
  /** Fits the composer toolbar instead of a page header. */
  compact?: boolean;
  className?: string;
}

const PROFILES: readonly TaskProfile[] = ['office', 'coding'] as const;

function isTaskProfile(value: string): value is TaskProfile {
  return value === 'office' || value === 'coding';
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
 * Work-mode control for choosing the Nomi profile. The selection is app-global
 * and persisted (see {@link useTaskProfileStore}) so it survives navigation
 * between modules and app restarts; a conversation freezes its own value at
 * creation.
 */
const TaskProfileSelector: React.FC<TaskProfileSelectorProps> = ({
  onProfileSelect,
  disabled = false,
  compact = false,
  className,
}) => {
  const { t } = useTranslation();
  const profile = useTaskProfileStore((state) => state.taskProfile);
  const setTaskProfile = useTaskProfileStore((state) => state.setTaskProfile);
  const [open, setOpen] = useState(false);

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

  const menu = (
    <Menu
      className={dropdownMenuStyles.configDropdownMenu}
      selectedKeys={[profile]}
      data-testid='task-profile-dropdown-menu'
      onClickMenuItem={(key) => {
        setOpen(false);
        if (!isTaskProfile(key)) return;
        handleSelect(key);
      }}
    >
      {PROFILES.map((value) => (
        <Menu.Item key={value}>
          <span className={styles.menuItem} data-testid={`task-profile-option-${value}`}>
            <span className={styles.optionIcon}>{profileIcon(value)}</span>
            <span>{profileLabel(value)}</span>
          </span>
        </Menu.Item>
      ))}
    </Menu>
  );

  return (
    <span className={[compact ? styles.switchCompact : '', className].filter(Boolean).join(' ')}>
      <Dropdown
        trigger='click'
        position='tl'
        disabled={disabled}
        popupVisible={open}
        onVisibleChange={(visible) => {
          if (!disabled) setOpen(visible);
        }}
        droplist={menu}
      >
        <button
          type='button'
          className={styles.trigger}
          data-button-shape='pill'
          data-testid='task-profile-selector'
          aria-label={label}
          aria-haspopup='menu'
          aria-expanded={open}
          disabled={disabled}
        >
          <span className={styles.optionIcon}>{profileIcon(profile)}</span>
          <span>{profileLabel(profile)}</span>
        </button>
      </Dropdown>
    </span>
  );
};

export default TaskProfileSelector;
