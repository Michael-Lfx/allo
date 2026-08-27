/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { IProvider } from '@/common/config/storage';
import { compositeKey } from '@/common/utils/compositeKey';
import ModelCreditRateHint from '@/renderer/components/model/ModelCreditRateHint';
import { useTranslation } from 'react-i18next';
import { Menu } from '@arco-design/web-react';
import React, { type CSSProperties } from 'react';
import {
  allChatModelOptions,
  AUTO_TIER_LABEL_FALLBACK,
  type AutoTier,
  type ChatModelOption,
  type ChatModelPickerViewModel,
} from '@/renderer/utils/model/chatModelPicker';

export interface ChatModelPickerMenuProps {
  viewModel: ChatModelPickerViewModel;
  selectedOption?: ChatModelOption;
  hasImageAttachments?: boolean;
  isLoading?: boolean;
  catalogError?: Error;
  onSelect: (option: ChatModelOption) => void;
  onRetry?: () => void;
  providerLabel: (provider: IProvider) => string;
}

export const FLOWY_AUTO_FAMILY_MENU_KEY = 'flowy-auto-family';

const menuStyle: CSSProperties = {
  width: 'min(300px, calc(100vw - 24px))',
  maxHeight: 'min(480px, max(160px, calc(100dvh - 96px)))',
};

const labelForTier = (
  tier: AutoTier | undefined,
  translate: (key: string, options?: { defaultValue: string }) => string,
): string =>
  tier
    ? translate(`conversation.modelPicker.autoTier.${tier}`, {
        defaultValue: AUTO_TIER_LABEL_FALLBACK[tier],
      })
    : translate('conversation.modelPicker.autoTier.unknown', { defaultValue: 'Auto' });

const healthDotColor = (option: ChatModelOption): string | null => {
  const status = option.health?.status ?? 'unknown';
  if (status === 'unknown') return null;
  return status === 'healthy' ? 'bg-green-500' : status === 'unhealthy' ? 'bg-red-500' : 'bg-gray-400';
};

const ChatModelPickerMenu: React.FC<ChatModelPickerMenuProps> = ({
  viewModel,
  selectedOption,
  hasImageAttachments = false,
  isLoading = false,
  catalogError,
  onSelect,
  onRetry,
  providerLabel,
}) => {
  const { t } = useTranslation();
  const options = allChatModelOptions(viewModel, { hasImageAttachments });
  const optionsByKey = new Map(options.map((option) => [option.key, option]));
  const autoOptions = options.filter((option) => option.family === 'auto');
  const cloudOptions = options.filter((option) => option.family === 'cloud');
  const hasModels = options.length > 0;
  const currentAutoOption =
    (selectedOption?.family === 'auto' ? optionsByKey.get(selectedOption.key) : undefined) ??
    autoOptions.find((option) => option.autoTier === 'balance') ??
    autoOptions[0];
  const autoTierForDisplay =
    selectedOption?.family === 'auto' ? currentAutoOption?.autoTier : currentAutoOption?.autoTier ?? 'balance';
  const selectedKey =
    selectedOption?.family === 'auto' ? FLOWY_AUTO_FAMILY_MENU_KEY : selectedOption?.key;

  const handleMenuItemClick = (key: string) => {
    const option =
      key === FLOWY_AUTO_FAMILY_MENU_KEY ? currentAutoOption : optionsByKey.get(key);
    if (!option || option.disabled) return;
    onSelect(option);
  };

  const modelRow = (option: ChatModelOption, testId: string) => {
    const isSelected = selectedOption?.key === option.key;
    const dot = healthDotColor(option);
    const fullLabel = option.label === option.model ? option.model : `${option.label} (${option.model})`;
    return (
      <Menu.Item
        key={option.key}
        data-testid={testId}
        className={`chat-model-picker-menu-item ${isSelected ? '!bg-2' : ''}`.trim()}
        disabled={option.disabled}
        aria-disabled={option.disabled}
        aria-selected={isSelected}
        aria-label={fullLabel}
        title={option.model}
      >
        <div className='flex min-w-0 w-full items-center justify-between gap-8px'>
          <div className='flex min-w-0 items-center gap-8px'>
            {dot && <span className={`h-6px w-6px shrink-0 rounded-full ${dot}`} aria-hidden='true' />}
            <span
              className='min-w-0 flex-1 truncate'
              title={option.model}
              aria-label={fullLabel}
            >
              {option.label}
            </span>
          </div>
          <span className='chat-model-picker-menu-meta flex w-52px shrink-0 justify-end'>
            <ModelCreditRateHint provider={option.provider} modelName={option.model} />
          </span>
        </div>
      </Menu.Item>
    );
  };

  return (
    <div
      className='chat-model-picker-menu flex min-h-0 flex-col overflow-hidden'
      style={menuStyle}
      data-testid='chat-model-picker-menu'
    >
      <Menu
        className='chat-model-picker-menu-list min-h-0 overflow-y-auto'
        style={{ maxHeight: menuStyle.maxHeight }}
        selectedKeys={selectedKey ? [selectedKey] : []}
        onClickMenuItem={handleMenuItemClick}
      >
        {isLoading && !catalogError && !hasModels && (
          <Menu.Item key='model-loading' disabled className='chat-model-picker-menu-item'>
            {t('common.loading')}
          </Menu.Item>
        )}

        {autoOptions.length > 0 && (
          <Menu.ItemGroup title={t('conversation.modelPicker.autoModels', { defaultValue: 'Auto models' })}>
            <Menu.Item
              key={FLOWY_AUTO_FAMILY_MENU_KEY}
              data-testid='chat-model-option-auto'
              className={`chat-model-picker-menu-item ${selectedOption?.family === 'auto' ? '!bg-2' : ''}`.trim()}
              disabled={hasImageAttachments || currentAutoOption?.disabled}
              aria-disabled={hasImageAttachments || currentAutoOption?.disabled}
              aria-selected={selectedOption?.family === 'auto'}
              aria-label={`${t('conversation.modelPicker.auto', { defaultValue: 'Auto' })} · ${labelForTier(
                autoTierForDisplay,
                t,
              )}`}
              title={
                hasImageAttachments
                  ? t('conversation.modelPicker.autoTextOnly', {
                      defaultValue: 'Auto models currently support text only',
                    })
                  : undefined
              }
            >
              <div className='flex min-w-0 w-full items-center justify-between gap-8px'>
                <span className='min-w-0 flex-1 truncate'>
                  {t('conversation.modelPicker.auto', { defaultValue: 'Auto' })}
                </span>
                <span className='chat-model-picker-menu-meta flex w-88px shrink-0 items-center justify-end gap-8px text-12px text-t-tertiary'>
                  <span className='truncate'>
                    {labelForTier(autoTierForDisplay, t)}
                  </span>
                  {selectedOption?.family === 'auto' && <span aria-hidden='true'>✓</span>}
                  <span aria-hidden='true'>›</span>
                </span>
              </div>
            </Menu.Item>
          </Menu.ItemGroup>
        )}

        {cloudOptions.length > 0 && (
          <Menu.ItemGroup title={t('conversation.modelPicker.cloudModels', { defaultValue: 'Cloud models' })}>
            {cloudOptions.map((option) => modelRow(option, `chat-model-option-${option.model}`))}
          </Menu.ItemGroup>
        )}

        {viewModel.otherProviderGroups.map((group) => {
          const groupOptions = group.models
            .map((model) => optionsByKey.get(compositeKey(group.provider.id, model)))
            .filter((option): option is ChatModelOption => Boolean(option));
          if (groupOptions.length === 0) return null;
          return (
            <Menu.ItemGroup title={providerLabel(group.provider)} key={group.provider.id}>
              {groupOptions.map((option) =>
                modelRow(option, `chat-model-option-${option.provider.id}-${option.model}`),
              )}
            </Menu.ItemGroup>
          );
        })}

        {catalogError && onRetry && (
          <Menu.Item key='model-retry' className='chat-model-picker-menu-status' onClick={onRetry}>
            {t('common.retry')}
          </Menu.Item>
        )}

        {!hasModels && !isLoading && !catalogError && (
          <Menu.Item key='model-empty' disabled className='chat-model-picker-menu-status'>
            {t('conversation.modelPicker.empty', { defaultValue: 'No models available' })}
          </Menu.Item>
        )}

        {!hasModels && catalogError && !onRetry && (
          <Menu.Item key='model-error' disabled className='chat-model-picker-menu-status'>
            {t('conversation.modelPicker.empty', { defaultValue: 'No models available' })}
          </Menu.Item>
        )}
      </Menu>
    </div>
  );
};

export default ChatModelPickerMenu;
