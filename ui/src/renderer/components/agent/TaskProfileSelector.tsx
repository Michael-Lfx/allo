/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Briefcase, Code, Ppt } from '@icon-park/react';
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
  /**
   * Optional controlled value. Omit to use the persisted global profile; the
   * Guid home uses it to show "coding" for the build capability without
   * mutating the app-global preference.
   */
  value?: TaskProfile;
  onChange?: (profile: TaskProfile) => void;
  /** Fits the composer toolbar instead of a page header. */
  compact?: boolean;
  className?: string;
  /**
   * Home composer only: adds a third "Slides" entry. Slides is a local mode of
   * the home page (ppt-master options), not a persisted work profile, so the
   * stored profile stays `office` while it is active.
   */
  slidesActive?: boolean;
  onSlidesChange?: (active: boolean) => void;
}

const PROFILES: readonly TaskProfile[] = ['office', 'coding'] as const;
const SLIDES_KEY = 'slides';

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
  value,
  onChange,
  compact = false,
  className,
  slidesActive = false,
  onSlidesChange,
}) => {
  const { t } = useTranslation();
  const storeProfile = useTaskProfileStore((state) => state.taskProfile);
  const setTaskProfile = useTaskProfileStore((state) => state.setTaskProfile);
  const profile = value ?? storeProfile;
  const [open, setOpen] = useState(false);

  const label = t('conversation.taskProfile.label', { defaultValue: '工作模式' });
  const officeLabel = t('conversation.taskProfile.office', { defaultValue: '日常办公' });
  const codingLabel = t('conversation.taskProfile.coding', { defaultValue: '代码开发' });

  const slidesLabel = t('presentation.home.label', { defaultValue: '幻灯片' });
  const showSlides = Boolean(onSlidesChange);
  const slidesSelected = showSlides && slidesActive;

  const profileLabel = (value: TaskProfile) => (value === 'coding' ? codingLabel : officeLabel);
  // The active profile is already shown on the chip; hide it from the menu.
  // When slides is active the stored profile is office but the visible mode is
  // slides, so both office and coding remain selectable to leave slides.
  const menuProfiles = PROFILES.filter((value) => (slidesActive ? true : value !== profile));

  const handleSelect = useCallback(
    (next: TaskProfile) => {
      if (disabled) return;
      if (slidesActive) onSlidesChange?.(false);
      const normalized = normalizeTaskProfile(next);
      if (normalized === profile) return;
      if (value === undefined) setTaskProfile(normalized);
      onChange?.(normalized);
      onProfileSelect?.(normalized);
    },
    [disabled, onProfileSelect, onSlidesChange, onChange, profile, setTaskProfile, slidesActive, value]
  );

  const handleSelectSlides = useCallback(() => {
    if (disabled || slidesActive) return;
    // Decks are office work: keep the controlled/global profile consistent.
    if (profile !== 'office') {
      if (value === undefined) setTaskProfile('office');
      onChange?.('office');
      onProfileSelect?.('office');
    }
    onSlidesChange?.(true);
  }, [disabled, onProfileSelect, onSlidesChange, onChange, profile, setTaskProfile, slidesActive, value]);

  const menu = (
    <Menu
      className={dropdownMenuStyles.configDropdownMenu}
      selectedKeys={[slidesSelected ? SLIDES_KEY : profile]}
      data-testid='task-profile-dropdown-menu'
      onClickMenuItem={(key) => {
        setOpen(false);
        if (key === SLIDES_KEY) {
          handleSelectSlides();
          return;
        }
        if (!isTaskProfile(key)) return;
        handleSelect(key);
      }}
    >
      {showSlides && (
        <Menu.Item key={SLIDES_KEY}>
          <span className={styles.menuItem} data-testid='task-profile-option-slides'>
            <span className={styles.optionIcon}>
              <Ppt theme='outline' size={16} fill='currentColor' />
            </span>
            <span>{slidesLabel}</span>
          </span>
        </Menu.Item>
      )}
      {menuProfiles.map((value) => (
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
          <span className={styles.optionIcon}>
            {slidesSelected ? <Ppt theme='outline' size={16} fill='currentColor' /> : profileIcon(profile)}
          </span>
          <span>{slidesSelected ? slidesLabel : profileLabel(profile)}</span>
        </button>
      </Dropdown>
    </span>
  );
};

export default TaskProfileSelector;
