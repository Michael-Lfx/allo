/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Button, Dropdown, Menu } from '@arco-design/web-react';
import { Down, Lightning } from '@icon-park/react';
import classNames from 'classnames';
import React, { useId, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useChatModelTriggerExpansion } from '@/renderer/components/model/useChatModelTriggerExpansion';
import type { AutoTier, ChatModelOption } from '@/renderer/utils/model/chatModelPicker';
import { AUTO_TIER_LABEL_FALLBACK, AUTO_TIER_ORDER } from '@/renderer/utils/model/chatModelPicker';

export interface AutoTierSelectorProps {
  options: readonly ChatModelOption[];
  selected?: ChatModelOption;
  hasImageAttachments?: boolean;
  disabled?: boolean;
  className?: string;
  /** Optional parent-owned popup state for mutually exclusive chat controls. */
  popupVisible?: boolean;
  onPopupVisibleChange?: (visible: boolean) => void;
  onSelect: (option: ChatModelOption) => Promise<void> | void;
}

const AutoTierSelector: React.FC<AutoTierSelectorProps> = ({
  options,
  selected,
  hasImageAttachments = false,
  disabled = false,
  className,
  popupVisible: popupVisibleProp,
  onPopupVisibleChange,
  onSelect,
}) => {
  const { t } = useTranslation();
  const [localPopupVisible, setLocalPopupVisible] = useState(false);
  const popupVisible = popupVisibleProp ?? localPopupVisible;
  const popupInstanceId = useId().replace(/:/g, '');
  const orderedOptions = useMemo(() => {
    const order = new Map(AUTO_TIER_ORDER.map((tier, index) => [tier, index]));
    return [...options].sort(
      (left, right) =>
        (left.autoTier ? order.get(left.autoTier) ?? AUTO_TIER_ORDER.length : AUTO_TIER_ORDER.length) -
          (right.autoTier ? order.get(right.autoTier) ?? AUTO_TIER_ORDER.length : AUTO_TIER_ORDER.length) ||
        left.label.localeCompare(right.label),
    );
  }, [options]);

  const labelForTier = (tier?: AutoTier): string =>
    tier
      ? t(`conversation.modelPicker.autoTier.${tier}`, {
          defaultValue: AUTO_TIER_LABEL_FALLBACK[tier],
        })
      : t('conversation.modelPicker.autoTier.unknown', { defaultValue: 'Auto' });

  const current =
    selected?.family === 'auto'
      ? selected
      : orderedOptions.find((option) => option.autoTier === 'balance') ?? orderedOptions[0];
  const strategyTriggerExpansion = useChatModelTriggerExpansion({
    enabled: !disabled && orderedOptions.length > 1,
    expandedWidth: 108,
    cssVariablePrefix: 'strategy',
    slotSelector: '.sendbox-strategy-slot',
    open: popupVisible,
  });
  if (!current) return null;

  const autoTierLabel = t('conversation.modelPicker.autoTierLabel', { defaultValue: 'Auto mode' });
  const autoTierTitle = t('conversation.modelPicker.autoTierTitle', { defaultValue: 'Auto mode' });
  const currentLabel = labelForTier(current.autoTier);
  const popupId = `auto-tier-selector-popup-${popupInstanceId}`;
  const handlePopupVisibleChange = (visible: boolean) => {
    if (popupVisibleProp === undefined) setLocalPopupVisible(visible);
    onPopupVisibleChange?.(visible);
  };

  return (
    <Dropdown
      trigger='click'
      getPopupContainer={() => document.body}
      popupVisible={popupVisible}
      onVisibleChange={handlePopupVisibleChange}
      droplist={
        <div
          id={popupId}
          className='min-w-180px'
          role='dialog'
          aria-label={autoTierTitle}
          data-testid='auto-tier-selector-popup'
          onClick={(event) => event.stopPropagation()}
        >
          <div className='px-12px pt-10px pb-6px text-12px text-t-tertiary'>{autoTierTitle}</div>
          <Menu selectedKeys={[current.key]}>
            {orderedOptions.map((option) => {
              const optionDisabled = disabled || (hasImageAttachments && !option.supportsVision);
              return (
                <Menu.Item
                  key={option.key}
                  disabled={optionDisabled}
                  aria-disabled={optionDisabled}
                  aria-label={`${labelForTier(option.autoTier)}: ${option.model}`}
                  title={option.model}
                  data-testid={`auto-tier-option-${option.autoTier ?? 'unknown'}`}
                  onClick={() => {
                    if (optionDisabled) return;
                    void Promise.resolve(onSelect(option)).catch((error) => {
                      console.error('[AutoTierSelector] Failed to select tier:', error);
                    });
                  }}
                >
                  <div className='flex items-center justify-between gap-12px w-full min-w-0'>
                    <span className='truncate min-w-0'>{labelForTier(option.autoTier)}</span>
                    {option.key === current.key && <span aria-hidden='true'>✓</span>}
                  </div>
                </Menu.Item>
              );
            })}
          </Menu>
          {hasImageAttachments && (
            <div className='px-12px pb-10px pt-6px text-12px text-t-tertiary'>
              {t('conversation.modelPicker.autoTextOnly', {
                defaultValue: 'Auto models currently support text only',
              })}
            </div>
          )}
        </div>
      }
    >
      <Button
        type='text'
        size='small'
        shape='round'
        ref={strategyTriggerExpansion.ref}
        style={strategyTriggerExpansion.style}
        disabled={disabled}
        className={classNames(
          'sendbox-responsive-reasoning-btn flowy-icon-text-btn',
          popupVisible && 'sendbox-responsive-control-open',
          className,
        )}
        aria-disabled={disabled || undefined}
        aria-label={`${autoTierLabel}: ${currentLabel}`}
        aria-haspopup='dialog'
        aria-controls={popupId}
        aria-expanded={popupVisible}
        data-popup-open={popupVisible ? 'true' : undefined}
        data-chat-strategy-expand-side={strategyTriggerExpansion.side}
        data-testid='auto-tier-selector'
        title={`${autoTierLabel}: ${currentLabel}`}
      >
        <span className='sendbox-responsive-leading-icon' data-layout-part='leading-icon'>
          <Lightning theme='filled' size='14' fill='currentColor' aria-hidden='true' />
        </span>
        <span className='sendbox-responsive-label auto-tier-trigger-label-slot'>
          <span className='auto-tier-trigger-label-reserve' aria-hidden='true'>
            {AUTO_TIER_ORDER.map((tier) => labelForTier(tier)).join(' / ')}
          </span>
          <span className='auto-tier-trigger-label-current'>{currentLabel}</span>
        </span>
        <span className='sendbox-responsive-chevron-slot' data-layout-part='chevron'>
          <Down
            theme='outline'
            size='11'
            fill='currentColor'
            className='sendbox-responsive-chevron shrink-0'
            aria-hidden='true'
          />
        </span>
      </Button>
    </Dropdown>
  );
};

export default AutoTierSelector;
