/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { ConversationId } from '@/common/types/ids';

import { ipcBridge } from '@/common';
import { iconColors } from '@/renderer/styles/colors';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import { Button, Dropdown, Menu, Message } from '@arco-design/web-react';
import { Down, Tips } from '@icon-park/react';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import classNames from 'classnames';
import dropdownMenuStyles from '@/renderer/styles/configDropdownMenu.module.css';

export interface ReasoningEffortSelectorProps {
  /** When set, selection is persisted to conversation.extra.reasoning_effort. */
  conversation_id?: ConversationId;
  /** Catalog-advertised effort levels for the active chat model. Empty → hide. */
  levels: readonly string[];
  /** Current / preferred effort (may be stale vs levels). */
  initialEffort?: string;
  disabled?: boolean;
  className?: string;
  /** Fired after a successful local change (and after conversation persist when applicable). */
  onEffortChanged?: (effort: string | undefined) => void;
}

const effortI18nKey = (level: string): string => `conversation.reasoningEffort.level.${level}`;

/**
 * Compact control for OpenAI-style `reasoning_effort`.
 * Shown only when the active Flowy catalog model advertises levels
 * (`extra.reasoning` + `extra.reasoning_effort`).
 *
 * Works in two modes:
 * - Session: `conversation_id` set → merge into conversation.extra
 * - Pre-session (Guid): no id → local-only via `onEffortChanged`
 */
const ReasoningEffortSelector: React.FC<ReasoningEffortSelectorProps> = ({
  conversation_id,
  levels,
  initialEffort,
  disabled = false,
  className,
  onEffortChanged,
}) => {
  const { t } = useTranslation();
  const layout = useLayoutContext();
  const isMobile = Boolean(layout?.isMobile);
  const [currentEffort, setCurrentEffort] = useState<string | undefined>(initialEffort);
  const [isSaving, setIsSaving] = useState(false);
  const [dropdownVisible, setDropdownVisible] = useState(false);

  const levelKey = levels.join('\0');

  const labelFor = useCallback(
    (level: string) =>
      t(effortI18nKey(level), {
        defaultValue: level,
      }),
    [t]
  );

  const longestLabel = useMemo(() => {
    let longest = '';
    for (const level of levels) {
      const label = labelFor(level);
      if (label.length > longest.length) {
        longest = label;
      }
    }
    return longest || t('acp.config.default');
  }, [labelFor, levels, t]);

  useEffect(() => {
    if (levels.length === 0) {
      setCurrentEffort(undefined);
      return;
    }
    if (initialEffort && levels.includes(initialEffort)) {
      setCurrentEffort(initialEffort);
      return;
    }
    const healed = levels.includes('medium') ? 'medium' : levels[0];
    setCurrentEffort(healed);
    if (initialEffort === healed) {
      return;
    }
    if (!conversation_id) {
      onEffortChanged?.(healed);
      return;
    }
    void ipcBridge.conversation.update
      .invoke({
        conversation_id,
        updates: { extra: { reasoning_effort: healed } },
        merge_extra: true,
      })
      .then((ok) => {
        if (ok) onEffortChanged?.(healed);
      })
      .catch(() => {
        /* best-effort heal */
      });
    // intentionally omit onEffortChanged — parent identity churn must not re-heal
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [conversation_id, initialEffort, levelKey]);

  const persistEffort = useCallback(
    async (effort: string) => {
      if (!conversation_id) {
        onEffortChanged?.(effort);
        return true;
      }
      const ok = await ipcBridge.conversation.update.invoke({
        conversation_id,
        updates: { extra: { reasoning_effort: effort } },
        merge_extra: true,
      });
      if (ok) onEffortChanged?.(effort);
      return Boolean(ok);
    },
    [conversation_id, onEffortChanged]
  );

  const handleSelect = useCallback(
    async (effort: string) => {
      setDropdownVisible(false);
      if (effort === currentEffort || disabled || isSaving) return;
      const previous = currentEffort;
      // Optimistic update keeps the pill width/label stable (no loading spinner).
      setCurrentEffort(effort);
      setIsSaving(true);
      try {
        const ok = await persistEffort(effort);
        if (!ok) {
          setCurrentEffort(previous);
          Message.error(t('conversation.reasoningEffort.switchFailed'));
        }
      } catch (error) {
        setCurrentEffort(previous);
        console.error('[ReasoningEffortSelector] Failed to update effort:', error);
        Message.error(t('conversation.reasoningEffort.switchFailed'));
      } finally {
        setIsSaving(false);
      }
    },
    [currentEffort, disabled, isSaving, persistEffort, t]
  );

  if (levels.length === 0) {
    return null;
  }

  const currentLabel = currentEffort ? labelFor(currentEffort) : t('acp.config.default');

  const dropdownMenu = (
    <Menu
      className={dropdownMenuStyles.configDropdownMenu}
      data-testid='reasoning-effort-dropdown-menu'
      onClickMenuItem={(key) => void handleSelect(key)}
    >
      <Menu.ItemGroup title={t('acp.config.reasoning_effort')}>
        {levels.map((level) => (
          <Menu.Item key={level} data-testid={`reasoning-effort-option-${level}`}>
            <div className='flex w-full items-center gap-8px' data-effort-value={level}>
              <span className='inline-flex w-14px shrink-0 justify-center text-primary'>
                {currentEffort === level ? '✓' : null}
              </span>
              <span>{labelFor(level)}</span>
            </div>
          </Menu.Item>
        ))}
      </Menu.ItemGroup>
    </Menu>
  );

  return (
    <Dropdown
      trigger='click'
      droplist={dropdownMenu}
      disabled={disabled || isSaving}
      popupVisible={dropdownVisible}
      onVisibleChange={setDropdownVisible}
      {...(isMobile ? { getPopupContainer: () => document.body } : {})}
    >
      <Button
        data-testid='reasoning-effort-selector'
        className={classNames(
          'sendbox-model-btn header-model-btn nomi-sendbox-reasoning-effort-btn min-w-0',
          className
        )}
        shape='round'
        size='small'
        disabled={disabled}
        aria-label={t('acp.config.reasoning_effort')}
      >
        <span className='flex items-center gap-6px min-w-0'>
          <Tips theme='outline' size='14' fill={iconColors.secondary} className='shrink-0' />
          {/* Grid stack: invisible longest label reserves width so switches don't jump. */}
          <span className='sendbox-responsive-label grid min-w-0 overflow-hidden [&>*]:col-start-1 [&>*]:row-start-1'>
            <span className='invisible whitespace-nowrap' aria-hidden>
              {longestLabel}
            </span>
            <span className='truncate text-center'>{currentLabel}</span>
          </span>
          <Down
            theme='outline'
            size={12}
            fill={iconColors.secondary}
            className='sendbox-responsive-chevron shrink-0'
          />
        </span>
      </Button>
    </Dropdown>
  );
};

export default ReasoningEffortSelector;
