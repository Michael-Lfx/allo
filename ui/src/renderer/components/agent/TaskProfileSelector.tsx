/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Button, Dropdown, Menu } from '@arco-design/web-react';
import { Down } from '@icon-park/react';
import classNames from 'classnames';
import React, { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import dropdownMenuStyles from '@/renderer/styles/configDropdownMenu.module.css';
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

/**
 * Single dropdown pill for choosing Nomi work mode before a conversation starts.
 * Mid-session switching is intentionally unsupported — profile is fixed at create.
 */
const TaskProfileSelector: React.FC<TaskProfileSelectorProps> = ({
  initialProfile = 'office',
  onProfileSelect,
  disabled = false,
  className,
}) => {
  const { t } = useTranslation();
  const [profile, setProfile] = useState<TaskProfile>(() => normalizeProfile(initialProfile));
  const [dropdownVisible, setDropdownVisible] = useState(false);

  const label = t('conversation.taskProfile.label', { defaultValue: '工作模式' });
  const officeLabel = t('conversation.taskProfile.office', { defaultValue: '日常办公' });
  const codingLabel = t('conversation.taskProfile.coding', { defaultValue: '代码开发' });

  const profileLabel = (value: TaskProfile) =>
    value === 'coding' ? codingLabel : officeLabel;

  useEffect(() => {
    setProfile(normalizeProfile(initialProfile));
  }, [initialProfile]);

  const handleSelect = useCallback(
    (key: string) => {
      setDropdownVisible(false);
      const next = normalizeProfile(key);
      if (disabled || next === profile) return;
      setProfile(next);
      onProfileSelect?.(next);
    },
    [disabled, onProfileSelect, profile]
  );

  const menu = (
    <Menu
      className={dropdownMenuStyles.configDropdownMenu}
      data-testid='task-profile-dropdown-menu'
      onClickMenuItem={handleSelect}
    >
      <Menu.ItemGroup title={label}>
        {PROFILES.map((value) => (
          <Menu.Item key={value} data-testid={`task-profile-option-${value}`}>
            <span className={profile === value ? styles.menuItemActive : undefined}>
              {profileLabel(value)}
            </span>
          </Menu.Item>
        ))}
      </Menu.ItemGroup>
    </Menu>
  );

  return (
    <Dropdown
      droplist={menu}
      trigger='click'
      position='bl'
      disabled={disabled}
      popupVisible={dropdownVisible}
      onVisibleChange={setDropdownVisible}
    >
      <Button
        type='secondary'
        shape='round'
        data-button-shape='pill'
        className={classNames(styles.trigger, className)}
        disabled={disabled}
        data-testid='task-profile-selector'
        aria-label={label}
      >
        <span className={styles.triggerLabel}>{profileLabel(profile)}</span>
        <Down theme='outline' size={12} fill='currentColor' />
      </Button>
    </Dropdown>
  );
};

export default TaskProfileSelector;
