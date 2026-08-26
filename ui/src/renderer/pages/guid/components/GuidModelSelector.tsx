/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { IProvider, TProviderWithModel } from '@/common/config/storage';
import { isManagedModelProvider } from '@/common/types/provider/managedModelService';
import { compositeKey } from '@/common/utils/compositeKey';
import { modelHealthOf } from '@/common/utils/providerModels';
import { iconColors } from '@/renderer/styles/colors';
import { getModelDisplayLabel } from '@/renderer/utils/model/agentLogo';
import type { AcpModelInfo } from '../types';
import { Button, Dropdown, Input, Menu } from '@arco-design/web-react';
import { Brain, Down } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import { useProvidersQuery } from '@/renderer/hooks/agent/useModelProviderList';
import { useModelsForTask } from '@/renderer/hooks/agent/useModelsForTask';
import { useModelSelectorProviderLabel } from '@/renderer/hooks/agent/useModelSelectorProviderLabel';
import { formatModelLabelForProvider } from '@/renderer/utils/model/cloudModelLabel';
import ModelCreditRateHint from '@/renderer/components/model/ModelCreditRateHint';
import { findChatModelForMenuKey } from './guidModelMenu';
import {
  allChatModelOptions,
  buildChatModelPickerViewModel,
  findChatModelOption,
  type ChatModelOption,
  type ChatModelPickerViewModel,
} from '@/renderer/utils/model/chatModelPicker';

type GuidModelSelectorProps = {
  // Gemini model state
  isGeminiMode: boolean;
  modelList: IProvider[];
  current_model: TProviderWithModel | undefined;
  defaultModelUnavailable?: boolean;
  setCurrentModel: (model: TProviderWithModel) => Promise<void>;
  modelPicker?: ChatModelPickerViewModel;
  hasImageAttachments?: boolean;
  isModelCatalogLoading?: boolean;
  modelCatalogError?: Error;
  refreshModelCatalog?: () => void;

  // ACP model state
  currentAcpCachedModelInfo: AcpModelInfo | null;
  selectedAcpModel: string | null;
  setSelectedAcpModel: React.Dispatch<React.SetStateAction<string | null>>;
};

type GuidModelSelectorButtonProps = {
  label: string;
  includeChevron?: boolean;
  readOnly?: boolean;
  labelClassName?: string;
} & Omit<React.ComponentProps<typeof Button>, 'children' | 'shape' | 'size'>;

export const GuidModelSelectorButton = React.forwardRef<HTMLButtonElement, GuidModelSelectorButtonProps>(
  ({ label, includeChevron = true, readOnly = false, labelClassName, className, style, ...rest }, ref) => (
    <Button
      ref={ref}
      {...rest}
      className={`sendbox-model-btn guid-config-btn flowy-icon-text-btn ${className ?? ''}`.trim()}
      shape='round'
      size='small'
      data-testid='guid-model-selector'
      style={readOnly ? { cursor: 'default', ...style } : style}
      aria-label={label}
    >
      <span className='flowy-button-inline-content flex items-center gap-6px min-w-0'>
        <Brain theme='outline' size='14' fill={iconColors.secondary} className='shrink-0' />
        <span className={`sendbox-responsive-label ${labelClassName ?? ''}`.trim()}>{label}</span>
        {includeChevron && (
          <Down
            theme='outline'
            size='12'
            fill={iconColors.secondary}
            className='sendbox-responsive-chevron shrink-0'
          />
        )}
      </span>
    </Button>
  ),
);
GuidModelSelectorButton.displayName = 'GuidModelSelectorButton';

const GuidModelSelector: React.FC<GuidModelSelectorProps> = ({
  isGeminiMode,
  modelList,
  current_model,
  defaultModelUnavailable = false,
  setCurrentModel,
  modelPicker,
  hasImageAttachments = false,
  isModelCatalogLoading = false,
  modelCatalogError,
  refreshModelCatalog,
  currentAcpCachedModelInfo,
  selectedAcpModel,
  setSelectedAcpModel,
}) => {
  const { t } = useTranslation();
  const defaultModelLabel = t('common.defaultModel');
  const providerLabel = useModelSelectorProviderLabel();
  const [search, setSearch] = React.useState('');

  // 获取模型配置数据（包含健康状态）
  const { data: modelConfig } = useProvidersQuery();

  // 统一 chat catalog（后端 resolve，无名称启发式）。modelList 仅约束「允许哪些
  // 供应商」（如 nomi 模式排除 Google Auth）；模型清单一律来自 catalog 分组。
  const { groups: chatGroups } = useModelsForTask('chat');
  const effectiveModelPicker = modelPicker ?? buildChatModelPickerViewModel(chatGroups);

  // 过滤掉被禁用的 provider，且仅保留调用方允许的供应商
  const enabledGroups = React.useMemo(() => {
    const allowedIds = new Set(modelList.filter((p) => p.enabled !== false).map((p) => p.id));
    return chatGroups.filter(
      (group) => allowedIds.has(group.provider.id) && !isManagedModelProvider(group.provider),
    );
  }, [chatGroups, modelList]);

  const enabledProviderIds = React.useMemo(
    () => new Set(enabledGroups.map((group) => group.provider.id)),
    [enabledGroups]
  );
  const enabledPicker = React.useMemo<ChatModelPickerViewModel>(
    () => ({
      autoModels: effectiveModelPicker.autoModels.filter((option) => enabledProviderIds.has(option.provider.id)),
      cloudModels: effectiveModelPicker.cloudModels.filter((option) => enabledProviderIds.has(option.provider.id)),
      otherProviderGroups: effectiveModelPicker.otherProviderGroups.filter((group) => enabledProviderIds.has(group.provider.id)),
    }),
    [effectiveModelPicker, enabledProviderIds]
  );
  const selectedChatModelOption = findChatModelOption(
    enabledPicker,
    current_model?.id,
    current_model?.use_model,
    { hasImageAttachments }
  );
  const autoTierLabel = (tier: ChatModelOption['autoTier']) =>
    tier
      ? t(`conversation.modelPicker.autoTier.${tier}`, {
          defaultValue: tier === 'intelligence' ? 'Intelligence' : tier === 'cost' ? 'Cost' : 'Balance',
        })
      : t('conversation.modelPicker.autoTier.unknown', { defaultValue: 'Auto' });

  const geminiSelectedLabel = React.useMemo(() => {
    if (!current_model?.use_model) return '';
    const liveProvider =
      enabledGroups.find((group) => group.provider.id === current_model.id)?.provider ?? current_model;
    if (selectedChatModelOption?.family === 'auto') {
      return `${t('conversation.modelPicker.auto', { defaultValue: 'Auto' })} · ${autoTierLabel(
        selectedChatModelOption.autoTier
      )}`;
    }
    return selectedChatModelOption?.label || formatModelLabelForProvider(liveProvider, current_model.use_model);
  }, [current_model, enabledGroups, selectedChatModelOption, t]);

  const geminiButtonLabel = React.useMemo(() => {
    if (defaultModelUnavailable) return t('conversation.chat.defaultModelUnavailable');
    return getModelDisplayLabel({
      selected_value: current_model?.use_model,
      selectedLabel: geminiSelectedLabel,
      defaultModelLabel,
      fallbackLabel: defaultModelLabel,
    });
  }, [current_model?.use_model, defaultModelLabel, defaultModelUnavailable, geminiSelectedLabel, t]);

  const acpSelectedLabel = React.useMemo(() => {
    return (
      currentAcpCachedModelInfo?.available_models?.find((m) => m.id === selectedAcpModel)?.label ||
      currentAcpCachedModelInfo?.current_model_label ||
      currentAcpCachedModelInfo?.current_model_id ||
      ''
    );
  }, [
    currentAcpCachedModelInfo?.available_models,
    currentAcpCachedModelInfo?.current_model_id,
    currentAcpCachedModelInfo?.current_model_label,
    selectedAcpModel,
  ]);

  const acpButtonLabel = React.useMemo(() => {
    return getModelDisplayLabel({
      selected_value: selectedAcpModel || currentAcpCachedModelInfo?.current_model_id,
      selectedLabel: acpSelectedLabel,
      defaultModelLabel,
      fallbackLabel: defaultModelLabel,
    });
  }, [acpSelectedLabel, currentAcpCachedModelInfo?.current_model_id, defaultModelLabel, selectedAcpModel]);

  const handleChatMenuItem = React.useCallback(
    (key: string) => {
      const catalogOptions = allChatModelOptions(enabledPicker, { hasImageAttachments });
      const selected =
        key === 'flowy-auto-family'
          ? selectedChatModelOption?.family === 'auto'
            ? selectedChatModelOption
            : hasImageAttachments
              ? undefined
              : enabledPicker.autoModels.find((option) => option.autoTier === 'balance') ?? enabledPicker.autoModels[0]
          : catalogOptions.find((option) => option.key === key);
      // Keep the legacy lookup as a compatibility guard for provider groups
      // while all new rows use the same catalog option key.
      const legacyMatch = findChatModelForMenuKey(enabledGroups, key);
      if (selected?.disabled) return;
      if (!selected && !legacyMatch) return;
      const provider = selected?.provider ?? legacyMatch?.provider;
      const model = selected?.model ?? legacyMatch?.modelName;
      if (!provider || !model) return;
      void setCurrentModel({ ...provider, use_model: model }).catch((error) => {
        console.error('Failed to set current model:', error);
      });
    },
    [enabledPicker, enabledGroups, hasImageAttachments, selectedChatModelOption, setCurrentModel],
  );

  const normalizedSearch = search.trim().toLocaleLowerCase();
  const pickerOptions = allChatModelOptions(enabledPicker, { hasImageAttachments });
  const matchesSearch = (option: ChatModelOption): boolean => {
    if (!normalizedSearch) return true;
    return `${option.label} ${option.model} ${option.autoTier ? autoTierLabel(option.autoTier) : ''}`
      .toLocaleLowerCase()
      .includes(normalizedSearch);
  };
  const filteredAutoModels = pickerOptions.filter((option) => option.family === 'auto').filter(matchesSearch);
  const filteredCloudModels = pickerOptions.filter((option) => option.family === 'cloud').filter(matchesSearch);
  const filteredOtherGroups = enabledPicker.otherProviderGroups
    .map((group) => ({
      ...group,
      models: group.models.filter((model) => {
        if (!normalizedSearch) return true;
        const option = allChatModelOptions(
          { autoModels: [], cloudModels: [], otherProviderGroups: [{ ...group, models: [model] }] },
          { hasImageAttachments }
        )[0];
        return option ? matchesSearch(option) : `${model}`.toLocaleLowerCase().includes(normalizedSearch);
      }),
    }))
    .filter((group) => group.models.length > 0);
  const visibleCatalogOptions = allChatModelOptions(enabledPicker, { hasImageAttachments });

  if (isGeminiMode) {
    const hasModels =
      filteredAutoModels.length > 0 || filteredCloudModels.length > 0 || filteredOtherGroups.length > 0;

    const healthDotColor = (option: ChatModelOption): string | null => {
      const healthStatus = option.health?.status || 'unknown';
      if (healthStatus === 'unknown') return null;
      return healthStatus === 'healthy' ? 'bg-green-500' : healthStatus === 'unhealthy' ? 'bg-red-500' : 'bg-gray-400';
    };

    return (
      <Dropdown
        trigger='click'
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
            <Menu
              selectedKeys={current_model ? [compositeKey(current_model.id, current_model.use_model)] : []}
              onClickMenuItem={handleChatMenuItem}
            >
              {!hasModels ? (
                <Menu.Item
                  key='no-models'
                  className='px-12px py-12px text-t-secondary text-14px text-center flex justify-center items-center'
                  disabled={!modelCatalogError}
                  onClick={() => refreshModelCatalog?.()}
                >
                  {modelCatalogError
                    ? t('common.retry')
                    : isModelCatalogLoading
                      ? t('common.loading')
                      : t('conversation.modelPicker.noResults', { defaultValue: 'No models found' })}
                </Menu.Item>
              ) : (
                <>
                  {defaultModelUnavailable ? (
                    <Menu.Item key='unavailable-default' disabled className='text-12px text-t-secondary'>
                      {t('conversation.chat.defaultModelUnavailable')}
                    </Menu.Item>
                  ) : null}
                  {filteredAutoModels.length > 0 && (
                    <Menu.ItemGroup title={t('conversation.modelPicker.autoModels', { defaultValue: 'Auto models' })}>
                      <Menu.Item
                        key='flowy-auto-family'
                        disabled={hasImageAttachments}
                        className={selectedChatModelOption?.family === 'auto' ? '!bg-2' : ''}
                        title={
                          hasImageAttachments
                            ? t('conversation.modelPicker.autoTextOnly', {
                                defaultValue: 'Auto models currently support text only',
                              })
                            : undefined
                        }
                      >
                        <div className='flex items-center justify-between gap-12px w-full min-w-0'>
                          <span className='truncate min-w-0'>
                            {t('conversation.modelPicker.auto', { defaultValue: 'Auto' })}
                          </span>
                          <span className='shrink-0 text-t-tertiary text-12px flex items-center gap-8px'>
                            {autoTierLabel(
                              selectedChatModelOption?.family === 'auto'
                                ? selectedChatModelOption.autoTier
                                : 'balance'
                            )}
                            {selectedChatModelOption?.family === 'auto' && <span aria-hidden='true'>✓</span>}
                            <span aria-hidden='true'>›</span>
                          </span>
                        </div>
                      </Menu.Item>
                    </Menu.ItemGroup>
                  )}
                  {filteredCloudModels.length > 0 && (
                    <Menu.ItemGroup title={t('conversation.modelPicker.cloudModels', { defaultValue: 'Cloud models' })}>
                      {filteredCloudModels.map((option) => {
                        const dot = healthDotColor(option);
                        return (
                          <Menu.Item
                            key={option.key}
                            disabled={option.disabled}
                            className={current_model?.id === option.provider.id && current_model?.use_model === option.model ? '!bg-2' : ''}
                          >
                            <div className='flex items-center justify-between gap-12px w-full min-w-0'>
                              <div className='flex items-center gap-8px min-w-0'>
                                {dot && <div className={`w-6px h-6px rounded-full shrink-0 ${dot}`} />}
                                <span className='truncate min-w-0' title={option.model}>
                                  {option.label}
                                </span>
                              </div>
                              <ModelCreditRateHint provider={option.provider} modelName={option.model} />
                            </div>
                          </Menu.Item>
                        );
                      })}
                    </Menu.ItemGroup>
                  )}
                  {filteredOtherGroups.map((group) => (
                    <Menu.ItemGroup title={providerLabel(group.provider)} key={group.provider.id}>
                      {group.models.map((modelName) => {
                        const option = visibleCatalogOptions.find(
                          (candidate) => candidate.provider.id === group.provider.id && candidate.model === modelName
                        );
                        const dot = option ? healthDotColor(option) : null;
                        return (
                          <Menu.Item
                            key={compositeKey(group.provider.id, modelName)}
                            disabled={option?.disabled}
                            className={current_model?.id === group.provider.id && current_model?.use_model === modelName ? '!bg-2' : ''}
                          >
                            <div className='flex items-center justify-between gap-12px w-full min-w-0'>
                              <div className='flex items-center gap-8px min-w-0'>
                                {dot && <div className={`w-6px h-6px rounded-full shrink-0 ${dot}`} />}
                                <span className='truncate min-w-0'>{option?.label ?? formatModelLabelForProvider(group.provider, modelName)}</span>
                              </div>
                              <ModelCreditRateHint provider={group.provider} modelName={modelName} />
                            </div>
                          </Menu.Item>
                        );
                      })}
                    </Menu.ItemGroup>
                  ))}
                </>
              )}
            </Menu>
          </div>
        }
      >
        <GuidModelSelectorButton label={geminiButtonLabel} labelClassName='truncate' />
      </Dropdown>
    );
  }

  // ACP cached model selector
  if (currentAcpCachedModelInfo && currentAcpCachedModelInfo.available_models?.length > 0) {
    if (currentAcpCachedModelInfo.available_models.length > 0) {
      return (
        <Dropdown
          trigger='click'
          droplist={
            <Menu
              selectedKeys={selectedAcpModel ? [selectedAcpModel] : []}
              onClickMenuItem={(key) => setSelectedAcpModel(key)}
            >
              {currentAcpCachedModelInfo.available_models.map((model) => {
                // 获取模型健康状态
                const providerConfig = modelConfig?.find((p) => p.platform?.includes(''));
                const healthStatus = modelHealthOf(providerConfig, model.id)?.status || 'unknown';
                const healthColor =
                  healthStatus === 'healthy'
                    ? 'bg-green-500'
                    : healthStatus === 'unhealthy'
                      ? 'bg-red-500'
                      : 'bg-gray-400';

                return (
                  <Menu.Item
                    key={model.id}
                    className={model.id === selectedAcpModel ? '!bg-2' : ''}
                  >
                    <div className='flex items-center gap-8px w-full'>
                      {healthStatus !== 'unknown' && (
                        <div className={`w-6px h-6px rounded-full shrink-0 ${healthColor}`} />
                      )}
                      <span>{model.label}</span>
                    </div>
                  </Menu.Item>
                );
              })}
            </Menu>
          }
        >
          <GuidModelSelectorButton label={acpButtonLabel} />
        </Dropdown>
      );
    }

    return (
      <GuidModelSelectorButton label={acpButtonLabel} includeChevron={false} readOnly />
    );
  }

  // Fallback: no model switching
  return (
    <GuidModelSelectorButton label={defaultModelLabel} includeChevron={false} readOnly />
  );
};

export default GuidModelSelector;
