/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { IProvider, TProviderWithModel } from '@/common/config/storage';
import { isManagedModelProvider } from '@/common/types/provider/managedModelService';
import { modelHealthOf } from '@/common/utils/providerModels';
import { iconColors } from '@/renderer/styles/colors';
import { getModelDisplayLabel } from '@/renderer/utils/model/agentLogo';
import type { AcpModelInfo } from '../types';
import ChatModelPickerMenu from '@/renderer/components/model/ChatModelPickerMenu';
import { Button, Dropdown, Menu } from '@arco-design/web-react';
import { Brain, Down } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import { useProvidersQuery } from '@/renderer/hooks/agent/useModelProviderList';
import { useModelsForTask, type TaskModelGroup } from '@/renderer/hooks/agent/useModelsForTask';
import { useModelSelectorProviderLabel } from '@/renderer/hooks/agent/useModelSelectorProviderLabel';
import { mergeRefs } from '@/renderer/components/notifications';
import { useChatModelTriggerExpansion } from '@/renderer/components/model/useChatModelTriggerExpansion';
import { formatModelLabelForProvider } from '@/renderer/utils/model/cloudModelLabel';
import {
  AUTO_TIER_LABEL_FALLBACK,
  buildChatModelPickerViewModel,
  findChatModelOption,
  type AutoTier,
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
  popupVisible?: boolean;
  onPopupVisibleChange?: (visible: boolean) => void;

  // ACP model state
  currentAcpCachedModelInfo: AcpModelInfo | null;
  selectedAcpModel: string | null;
  setSelectedAcpModel: React.Dispatch<React.SetStateAction<string | null>>;
};

type GuidModelSelectorButtonProps = {
  label: string;
  includeChevron?: boolean;
  readOnly?: boolean;
  chatModelTrigger?: boolean;
  labelClassName?: string;
} & Omit<React.ComponentProps<typeof Button>, 'children' | 'shape' | 'size'>;

export const GuidModelSelectorButton = React.forwardRef<HTMLButtonElement, GuidModelSelectorButtonProps>(
  (
    {
      label,
      includeChevron = true,
      readOnly = false,
      chatModelTrigger = false,
      labelClassName,
      className,
      style,
      ...rest
    },
    ref,
  ) => {
    const modelTriggerExpansion = useChatModelTriggerExpansion({
      enabled: chatModelTrigger,
      open: rest['aria-expanded'] === true,
    });
    const buttonRef = chatModelTrigger
      ? mergeRefs(ref ?? undefined, modelTriggerExpansion.ref)
      : ref;

    return (
      <Button
        ref={buttonRef}
        {...rest}
        className={`sendbox-model-btn guid-config-btn flowy-icon-text-btn ${
          chatModelTrigger ? 'chat-model-picker-trigger' : ''
        } ${className ?? ''}`.trim()}
        shape='round'
        size='small'
        data-testid='guid-model-selector'
        style={
          chatModelTrigger
            ? { ...(readOnly ? { cursor: 'default' } : {}), ...style, ...modelTriggerExpansion.style }
            : readOnly
              ? { cursor: 'default', ...style }
              : style
        }
        data-chat-model-expand-side={chatModelTrigger ? modelTriggerExpansion.side : undefined}
        aria-label={label}
        title={label}
      >
        <span className='flowy-button-inline-content flex items-center gap-6px min-w-0'>
          <span className='sendbox-responsive-leading-icon' data-layout-part='leading-icon'>
            <Brain theme='outline' size='14' fill={iconColors.secondary} />
          </span>
          <span className={`sendbox-responsive-label ${labelClassName ?? ''}`.trim()}>{label}</span>
          {includeChevron && (
            <span className='sendbox-responsive-chevron-slot' data-layout-part='chevron'>
              <Down
                theme='outline'
                size='11'
                fill={iconColors.secondary}
                className='sendbox-responsive-chevron shrink-0'
              />
            </span>
          )}
        </span>
      </Button>
    );
  },
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
  popupVisible: popupVisibleProp,
  onPopupVisibleChange,
  currentAcpCachedModelInfo,
  selectedAcpModel,
  setSelectedAcpModel,
}) => {
  const { t } = useTranslation();
  const [localModelPickerOpen, setLocalModelPickerOpen] = React.useState(false);
  const modelPickerOpen = popupVisibleProp ?? localModelPickerOpen;
  const defaultModelLabel = t('common.defaultModel');
  const providerLabel = useModelSelectorProviderLabel();

  const handleModelPickerVisibleChange = (visible: boolean) => {
    if (popupVisibleProp === undefined) {
      setLocalModelPickerOpen(visible);
    }
    onPopupVisibleChange?.(visible);
  };

  // 获取模型配置数据（包含健康状态）
  const { data: modelConfig } = useProvidersQuery();

  // 统一 chat catalog（后端 resolve，无名称启发式）。modelList 仅约束「允许哪些
  // 供应商」（如 nomi 模式排除 Google Auth）；模型清单一律来自 catalog 分组。
  // GuidPage already owns the authoritative picker. Disable the fallback
  // catalog request there; cron and other legacy callers still get the hook.
  const { groups: chatGroups } = useModelsForTask('chat', undefined, { enabled: !modelPicker });
  const effectiveModelPicker = modelPicker ?? buildChatModelPickerViewModel(chatGroups);

  const pickerGroups = React.useMemo<TaskModelGroup[]>(() => {
    const byProvider = new Map<string, TaskModelGroup>();
    const add = (provider: IProvider, model: string) => {
      const existing = byProvider.get(provider.id);
      if (existing) {
        if (!existing.models.includes(model)) existing.models.push(model);
      } else {
        byProvider.set(provider.id, { provider, models: [model] });
      }
    };
    for (const option of [...effectiveModelPicker.autoModels, ...effectiveModelPicker.cloudModels]) {
      add(option.provider, option.model);
    }
    for (const group of effectiveModelPicker.otherProviderGroups) {
      for (const model of group.models) add(group.provider, model);
    }
    return [...byProvider.values()];
  }, [effectiveModelPicker]);

  // 过滤掉被禁用的 provider，且仅保留调用方允许的供应商
  const enabledGroups = React.useMemo(() => {
    const allowedIds = new Set(modelList.filter((p) => p.enabled !== false).map((p) => p.id));
    const catalogGroups = modelPicker ? pickerGroups : chatGroups;
    return catalogGroups.filter(
      (group) => allowedIds.has(group.provider.id) && !isManagedModelProvider(group.provider),
    );
  }, [chatGroups, modelList, modelPicker, pickerGroups]);

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
  const autoTierLabel = (tier: AutoTier | undefined) =>
    tier
      ? t(`conversation.modelPicker.autoTier.${tier}`, {
          defaultValue: AUTO_TIER_LABEL_FALLBACK[tier],
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

  if (isGeminiMode) {
    return (
      <Dropdown
        trigger='click'
        getPopupContainer={() => document.body}
        popupVisible={modelPickerOpen}
        onVisibleChange={handleModelPickerVisibleChange}
        droplist={
          <ChatModelPickerMenu
            viewModel={enabledPicker}
            selectedOption={selectedChatModelOption}
            hasImageAttachments={hasImageAttachments}
            isLoading={isModelCatalogLoading}
            catalogError={modelCatalogError}
            onSelect={(option) => {
              void setCurrentModel({ ...option.provider, use_model: option.model }).catch((error) => {
                console.error('Failed to set current model:', error);
              });
            }}
            onRetry={refreshModelCatalog}
            providerLabel={providerLabel}
          />
        }
      >
        <GuidModelSelectorButton
          label={geminiButtonLabel}
          chatModelTrigger
          className={modelPickerOpen ? 'sendbox-responsive-control-open' : undefined}
          aria-expanded={modelPickerOpen}
          data-popup-open={modelPickerOpen ? 'true' : undefined}
        />
      </Dropdown>
    );
  }

  // ACP cached model selector
  if (currentAcpCachedModelInfo && currentAcpCachedModelInfo.available_models?.length > 0) {
    if (currentAcpCachedModelInfo.available_models.length > 0) {
      return (
        <Dropdown
          trigger='click'
          getPopupContainer={() => document.body}
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
