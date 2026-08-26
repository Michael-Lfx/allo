

import type { NomiModelSelection } from './useNomiModelSelection';
import { usePreviewContext } from '@/renderer/pages/conversation/Preview';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import { getModelDisplayLabel } from '@/renderer/utils/model/agentLogo';
import { iconColors } from '@/renderer/styles/colors';
import ModelCreditRateHint from '@/renderer/components/model/ModelCreditRateHint';
import { Button, Dropdown, Input, Menu } from '@arco-design/web-react';
import { Brain, Down } from '@icon-park/react';
import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';
import classNames from 'classnames';
import { useModelSelectorProviderLabel } from '@/renderer/hooks/agent/useModelSelectorProviderLabel';
import {
  findChatModelOption,
  type AutoTier,
  type ChatModelOption,
  type ChatModelPickerViewModel,
} from '@/renderer/utils/model/chatModelPicker';

const EMPTY_MODEL_PICKER: ChatModelPickerViewModel = {
  autoModels: [],
  cloudModels: [],
  otherProviderGroups: [],
} as const;

const NomiModelSelector: React.FC<{
  selection?: NomiModelSelection;
  disabled?: boolean;
  hasImageAttachments?: boolean;
  compact?: boolean;
  className?: string;
}> = ({ selection, disabled = false, hasImageAttachments = false, compact: compactProp, className }) => {
  const { t } = useTranslation();
  const { isOpen: isPreviewOpen } = usePreviewContext();
  const layout = useLayoutContext();
  const compact = compactProp ?? (isPreviewOpen || layout?.isMobile);
  const isMobileHeaderCompact = Boolean(layout?.isMobile);
  const defaultModelLabel = t('common.defaultModel');
  const providerLabel = useModelSelectorProviderLabel();
  const [search, setSearch] = useState('');

  const current_model = selection?.current_model;
  const modelPicker = selection?.modelPicker ?? EMPTY_MODEL_PICKER;
  const isCatalogLoading = Boolean(selection?.isModelCatalogLoading);
  const formatModelLabel =
    selection?.formatModelLabel ??
    ((_provider: { model_descriptions?: Record<string, string> } | undefined, model?: string) => model ?? '');
  const getDisplayModelName = selection?.getDisplayModelName;
  const handleSelectModel = selection?.handleSelectModel ?? (async () => undefined);

  const renderLogo = () => <Brain theme='outline' size='14' fill={iconColors.secondary} className='shrink-0' />;

  const selectedLabel = getDisplayModelName?.(current_model?.use_model) ?? '';
  const selectedOption = findChatModelOption(modelPicker, current_model?.id, current_model?.use_model);

  const autoTierLabel = (tier?: AutoTier) =>
    tier
      ? t(`conversation.modelPicker.autoTier.${tier}`, {
          defaultValue: tier === 'intelligence' ? 'Intelligence' : tier === 'balance' ? 'Balance' : 'Cost',
        })
      : t('conversation.modelPicker.autoTier.unknown', { defaultValue: 'Auto' });

  const modelButtonLabel = selectedOption?.family === 'auto'
    ? `${t('conversation.modelPicker.auto', { defaultValue: 'Auto' })} · ${autoTierLabel(selectedOption.autoTier)}`
    : selectedOption?.label || selectedLabel;

  const label = getModelDisplayLabel({
    selected_value: current_model?.use_model,
    selectedLabel: modelButtonLabel,
    defaultModelLabel,
    fallbackLabel: t('conversation.welcome.selectModel'),
  });

  const normalizedSearch = search.trim().toLocaleLowerCase();
  const matchesSearch = (option: ChatModelOption) => {
    if (!normalizedSearch) return true;
    const tier = option.autoTier ? autoTierLabel(option.autoTier) : '';
    return `${option.label} ${option.model} ${tier}`.toLocaleLowerCase().includes(normalizedSearch);
  };

  const isDisabled = (option: ChatModelOption) => hasImageAttachments && !option.supportsVision;

  const autoOptions = modelPicker.autoModels.filter((option) => matchesSearch(option));
  const cloudOptions = modelPicker.cloudModels.filter((option) => matchesSearch(option));
  const otherGroups = modelPicker.otherProviderGroups
    .map((group) => ({
      ...group,
      models: group.models.filter((model) => {
        if (!normalizedSearch) return true;
        const label = formatModelLabel(group.provider, model);
        return `${label} ${model}`.toLocaleLowerCase().includes(normalizedSearch);
      }),
    }))
    .filter((group) => group.models.length > 0);

  const hasResults = autoOptions.length > 0 || cloudOptions.length > 0 || otherGroups.length > 0;

  const selectOption = async (option: ChatModelOption) => {
    if (isDisabled(option)) return;
    await handleSelectModel(option.provider, option.model);
  };

  const selectCurrentAuto = async () => {
    const currentAuto =
      selectedOption?.family === 'auto'
        ? selectedOption
        : modelPicker.autoModels.find((option) => option.autoTier === 'balance') ?? modelPicker.autoModels[0];
    if (currentAuto) await selectOption(currentAuto);
  };

  if (disabled || !selection) {
    return (
      <Button
        data-testid='nomi-model-selector'
        className={classNames(
          'sendbox-model-btn header-model-btn min-w-0',
          'flowy-icon-text-btn',
          compact ? '!max-w-[120px]' : '!max-w-[280px]',
          isMobileHeaderCompact && '!max-w-[160px]',
          className
        )}
        shape='round'
        size='small'
        loading={selection?.isModelCatalogLoading}
        style={{ cursor: 'default' }}
        aria-label={
          selection?.isModelCatalogLoading
            ? t('common.loading')
            : t('conversation.welcome.useCliModel')
        }
      >
        <span className='flowy-button-inline-content flex items-center gap-6px min-w-0'>
          {renderLogo()}
          <span className='sendbox-responsive-label block truncate min-w-0'>
            {t('conversation.welcome.useCliModel')}
          </span>
        </span>
      </Button>
    );
  }

  return (
    <Dropdown
      trigger='click'
      // Mobile: portal the popup to <body> so it escapes the titlebar slot.
      // Desktop: leave default container so click events reach Menu.Item normally.
      {...(isMobileHeaderCompact ? { getPopupContainer: () => document.body } : {})}
      onVisibleChange={(visible) => {
        if (!visible) setSearch('');
      }}
      droplist={
        <div className='w-360px max-w-[calc(100vw-20px)]'>
          <div className='px-10px pt-8px pb-4px'>
            <Input
              allowClear
              size='small'
              value={search}
              onChange={setSearch}
              placeholder={t('conversation.modelPicker.search', { defaultValue: 'Search models' })}
              aria-label={t('conversation.modelPicker.search', { defaultValue: 'Search models' })}
            />
          </div>
          <Menu selectedKeys={selectedOption ? [selectedOption.key] : []}>
            {isCatalogLoading && !selection?.modelCatalogError && !hasResults && (
              <Menu.Item key='model-loading' disabled>
                {t('common.loading')}
              </Menu.Item>
            )}
            {autoOptions.length > 0 && (
              <Menu.ItemGroup title={t('conversation.modelPicker.autoModels', { defaultValue: 'Auto models' })}>
                <Menu.Item
                  key={selectedOption?.family === 'auto' ? selectedOption.key : 'flowy-auto'}
                  data-testid='nomi-model-option-auto'
                  className={selectedOption?.family === 'auto' ? '!bg-2' : ''}
                  disabled={hasImageAttachments}
                  title={
                    hasImageAttachments
                      ? t('conversation.modelPicker.autoTextOnly', {
                          defaultValue: 'Auto models currently support text only',
                        })
                      : undefined
                  }
                  onClick={() => void selectCurrentAuto()}
                >
                  <div className='flex items-center justify-between gap-12px w-full min-w-0'>
                    <span className='truncate min-w-0'>
                      {t('conversation.modelPicker.auto', { defaultValue: 'Auto' })}
                    </span>
                    <span className='shrink-0 text-t-tertiary text-12px flex items-center gap-8px'>
                      {autoTierLabel(selectedOption?.family === 'auto' ? selectedOption.autoTier : 'balance')}
                      {selectedOption?.family === 'auto' && <span aria-hidden='true'>✓</span>}
                      <span aria-hidden='true'>›</span>
                    </span>
                  </div>
                </Menu.Item>
              </Menu.ItemGroup>
            )}
            {cloudOptions.length > 0 && (
              <Menu.ItemGroup title={t('conversation.modelPicker.cloudModels', { defaultValue: 'Cloud models' })}>
                {cloudOptions.map((option) => {
                  const optionDisabled = isDisabled(option);
                  return (
                    <Menu.Item
                      key={option.key}
                      data-testid={`nomi-model-option-${option.model}`}
                      className={current_model?.id === option.provider.id && current_model?.use_model === option.model ? '!bg-2' : ''}
                      disabled={optionDisabled}
                      onClick={() => void selectOption(option)}
                    >
                      <div className='flex items-center justify-between gap-12px w-full min-w-0'>
                        <span className='truncate min-w-0' title={option.model}>
                          {option.label}
                        </span>
                        <ModelCreditRateHint provider={option.provider} modelName={option.model} />
                      </div>
                    </Menu.Item>
                  );
                })}
              </Menu.ItemGroup>
            )}
            {otherGroups.map((group) => (
              <Menu.ItemGroup title={providerLabel(group.provider)} key={group.provider.id}>
                {group.models.map((modelName) => (
                  <Menu.Item
                    key={`${group.provider.id}:${modelName}`}
                    data-testid={`nomi-model-option-${modelName}`}
                    className={current_model?.id === group.provider.id && current_model?.use_model === modelName ? '!bg-2' : ''}
                    title={modelName}
                    onClick={() => void handleSelectModel(group.provider, modelName)}
                  >
                    <div className='flex items-center justify-between gap-12px w-full min-w-0'>
                      <span className='truncate min-w-0'>{formatModelLabel(group.provider, modelName)}</span>
                      <ModelCreditRateHint provider={group.provider} modelName={modelName} />
                    </div>
                  </Menu.Item>
                ))}
              </Menu.ItemGroup>
            ))}
            {!hasResults && (!isCatalogLoading || selection?.modelCatalogError) && (
              <Menu.Item
                key='no-model-results'
                disabled={!selection?.modelCatalogError}
                onClick={() => selection?.refreshModelCatalog()}
              >
                {selection?.modelCatalogError
                  ? t('common.retry')
                  : t('conversation.modelPicker.noResults', { defaultValue: 'No models found' })}
              </Menu.Item>
            )}
          </Menu>
        </div>
      }
    >
      <Button
        data-testid='nomi-model-selector'
        className={classNames(
          'sendbox-model-btn header-model-btn min-w-0',
          'flowy-icon-text-btn',
          compact ? '!max-w-[120px]' : '!max-w-[280px]',
          isMobileHeaderCompact && '!max-w-[160px]',
          className
        )}
        shape='round'
        size='small'
        aria-label={label}
      >
        <span className='flowy-button-inline-content flex items-center gap-6px min-w-0'>
          {renderLogo()}
          <span className='sendbox-responsive-label block truncate min-w-0'>{label}</span>
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

export default NomiModelSelector;
