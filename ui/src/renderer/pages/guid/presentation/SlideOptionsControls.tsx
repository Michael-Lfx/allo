/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dropdown, Menu } from '@arco-design/web-react';
import { Down, Up } from '@icon-park/react';
import dropdownMenuStyles from '@/renderer/styles/configDropdownMenu.module.css';
import { SLIDE_CANVASES, SLIDE_MODES, SLIDE_PAGES, fileBaseName, type SlideOptions, type SlideTemplate } from './slideOptions';
import styles from './SlideOptionsControls.module.css';

type OptionPillProps<T extends string> = {
  value: T;
  values: readonly T[];
  label: (value: T) => string;
  ariaLabel: string;
  testId: string;
  disabled?: boolean;
  onChange: (value: T) => void;
};

function OptionPill<T extends string>({ value, values, label, ariaLabel, testId, disabled, onChange }: OptionPillProps<T>) {
  const [open, setOpen] = useState(false);
  const menu = (
    <Menu
      className={dropdownMenuStyles.configDropdownMenu}
      selectedKeys={[value]}
      onClickMenuItem={(key) => {
        setOpen(false);
        const next = values.find((candidate) => candidate === key);
        if (next !== undefined && next !== value) onChange(next);
      }}
    >
      {values.map((candidate) => (
        <Menu.Item key={candidate} data-testid={`${testId}-${candidate}`}>
          {label(candidate)}
        </Menu.Item>
      ))}
    </Menu>
  );
  return (
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
        className={styles.pill}
        data-button-shape='pill'
        data-testid={testId}
        aria-label={ariaLabel}
        aria-haspopup='menu'
        aria-expanded={open}
        disabled={disabled}
      >
        <span>{label(value)}</span>
        <span className={styles.chevron} aria-hidden='true'>
          <Down theme='outline' size={12} fill='currentColor' />
        </span>
      </button>
    </Dropdown>
  );
}

type SlideOptionsControlsProps = {
  options: SlideOptions;
  onChange: (options: SlideOptions) => void;
  disabled?: boolean;
  /** Template pill: shows the current choice and toggles the create panel. */
  templatesOpen?: boolean;
  onToggleTemplates?: () => void;
};

function useTemplateLabel(): (template: SlideTemplate) => string {
  const { t } = useTranslation();
  return (template) => {
    switch (template.kind) {
      case 'free':
        return t('presentation.create.free.name');
      case 'theme':
        return t(`presentation.create.themes.${template.id}.name`);
      case 'import':
        return fileBaseName(template.path);
    }
  };
}

/**
 * Canvas / length / pace pills in the home composer's bottom toolbar, shown
 * only while the work-mode selector is on "Slides".
 */
const SlideOptionsControls: React.FC<SlideOptionsControlsProps> = ({
  options,
  onChange,
  disabled,
  templatesOpen = false,
  onToggleTemplates,
}) => {
  const { t } = useTranslation();
  const templateLabel = useTemplateLabel();
  return (
    <span className={styles.group} data-testid='guid-slide-options'>
      {onToggleTemplates && (
        <button
          type='button'
          className={styles.pill}
          data-button-shape='pill'
          data-testid='guid-slide-template'
          aria-label={t('presentation.create.templateTitle')}
          aria-expanded={templatesOpen}
          title={t('presentation.create.templateTitle')}
          disabled={disabled}
          onClick={onToggleTemplates}
        >
          <span className={styles.label}>{templateLabel(options.template)}</span>
          <span className={styles.chevron} aria-hidden='true'>
            {templatesOpen ? (
              <Down theme='outline' size={12} fill='currentColor' />
            ) : (
              <Up theme='outline' size={12} fill='currentColor' />
            )}
          </span>
        </button>
      )}
      <OptionPill
        value={options.canvas}
        values={SLIDE_CANVASES}
        label={(value) => t(`presentation.home.canvases.${value}`)}
        ariaLabel={t('presentation.home.canvas')}
        testId='guid-slide-canvas'
        disabled={disabled}
        onChange={(canvas) => onChange({ ...options, canvas })}
      />
      <OptionPill
        value={options.pages}
        values={SLIDE_PAGES}
        label={(value) => t(`presentation.home.pageCounts.${value}`)}
        ariaLabel={t('presentation.home.pages')}
        testId='guid-slide-pages'
        disabled={disabled}
        onChange={(pages) => onChange({ ...options, pages })}
      />
      <OptionPill
        value={options.mode}
        values={SLIDE_MODES}
        label={(value) => t(`presentation.home.modes.${value}`)}
        ariaLabel={t('presentation.home.mode')}
        testId='guid-slide-mode'
        disabled={disabled}
        onChange={(mode) => onChange({ ...options, mode })}
      />
    </span>
  );
};

export default SlideOptionsControls;
