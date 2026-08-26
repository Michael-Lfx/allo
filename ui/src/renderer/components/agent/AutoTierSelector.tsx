/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Button, Dropdown, Menu } from '@arco-design/web-react';
import { Down, Lightning } from '@icon-park/react';
import classNames from 'classnames';
import React, { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { AutoTier, ChatModelOption } from '@/renderer/utils/model/chatModelPicker';
import { AUTO_TIER_ORDER } from '@/renderer/utils/model/chatModelPicker';

export interface AutoTierSelectorProps {
  options: readonly ChatModelOption[];
  selected?: ChatModelOption;
  hasImageAttachments?: boolean;
  disabled?: boolean;
  className?: string;
  onSelect: (option: ChatModelOption) => Promise<void> | void;
}

const tierLabelFallback: Record<AutoTier, string> = {
  intelligence: 'Intelligence',
  balance: 'Balance',
  cost: 'Cost',
};

const AutoTierSelector: React.FC<AutoTierSelectorProps> = ({
  options,
  selected,
  hasImageAttachments = false,
  disabled = false,
  className,
  onSelect,
}) => {
  const { t } = useTranslation();
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
          defaultValue: tierLabelFallback[tier],
        })
      : t('conversation.modelPicker.autoTier.unknown', { defaultValue: 'Auto' });

  const current =
    selected?.family === 'auto'
      ? selected
      : orderedOptions.find((option) => option.autoTier === 'balance') ?? orderedOptions[0];
  if (!current) return null;

  const autoTierLabel = t('conversation.modelPicker.autoTierLabel', { defaultValue: 'Tier' });
  const autoTierTitle = t('conversation.modelPicker.autoTierTitle', { defaultValue: 'Auto tier' });
  const currentLabel = labelForTier(current.autoTier);
  const popupId = 'auto-tier-selector-popup';

  return (
    <Dropdown
      trigger='click'
      getPopupContainer={() => document.body}
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
        disabled={disabled}
        className={classNames('sendbox-responsive-reasoning-btn', className)}
        aria-label={`${autoTierLabel}: ${currentLabel}`}
        aria-haspopup='dialog'
        aria-controls={popupId}
        data-testid='auto-tier-selector'
        title={`${autoTierLabel}: ${currentLabel}`}
      >
        <Lightning theme='filled' size='14' fill='currentColor' aria-hidden='true' />
        <span className='sendbox-responsive-label'>{currentLabel}</span>
        <Down theme='outline' size='11' fill='currentColor' aria-hidden='true' />
      </Button>
    </Dropdown>
  );
};

export default AutoTierSelector;
