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
import { Button, Dropdown, Menu } from '@arco-design/web-react';
import { Brain, Down } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import { useProvidersQuery } from '@/renderer/hooks/agent/useModelProviderList';
import { useModelsForTask } from '@/renderer/hooks/agent/useModelsForTask';
import { useModelSelectorProviderLabel } from '@/renderer/hooks/agent/useModelSelectorProviderLabel';
import { formatModelLabelForProvider } from '@/renderer/utils/model/cloudModelLabel';
import ModelCreditRateHint from '@/renderer/components/model/ModelCreditRateHint';

type GuidModelSelectorProps = {
  // Gemini model state
  isGeminiMode: boolean;
  modelList: IProvider[];
  current_model: TProviderWithModel | undefined;
  defaultModelUnavailable?: boolean;
  setCurrentModel: (model: TProviderWithModel) => Promise<void>;

  // ACP model state
  currentAcpCachedModelInfo: AcpModelInfo | null;
  selectedAcpModel: string | null;
  setSelectedAcpModel: React.Dispatch<React.SetStateAction<string | null>>;
};

export const GuidModelSelectorButton: React.FC<{
  label: string;
  includeChevron?: boolean;
  readOnly?: boolean;
  labelClassName?: string;
}> = ({ label, includeChevron = true, readOnly = false, labelClassName }) => (
  <Button
    className='sendbox-model-btn guid-config-btn flowy-icon-text-btn'
    shape='round'
    size='small'
    data-testid='guid-model-selector'
    style={readOnly ? { cursor: 'default' } : undefined}
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
);

const GuidModelSelector: React.FC<GuidModelSelectorProps> = ({
  isGeminiMode,
  modelList,
  current_model,
  defaultModelUnavailable = false,
  setCurrentModel,
  currentAcpCachedModelInfo,
  selectedAcpModel,
  setSelectedAcpModel,
}) => {
  const { t } = useTranslation();
  const defaultModelLabel = t('common.defaultModel');
  const providerLabel = useModelSelectorProviderLabel();

  // 获取模型配置数据（包含健康状态）
  const { data: modelConfig } = useProvidersQuery();

  // 统一 chat catalog（后端 resolve，无名称启发式）。modelList 仅约束「允许哪些
  // 供应商」（如 nomi 模式排除 Google Auth）；模型清单一律来自 catalog 分组。
  const { groups: chatGroups } = useModelsForTask('chat');

  // 过滤掉被禁用的 provider，且仅保留调用方允许的供应商
  const enabledGroups = React.useMemo(() => {
    const allowedIds = new Set(modelList.filter((p) => p.enabled !== false).map((p) => p.id));
    return chatGroups.filter(
      (group) => allowedIds.has(group.provider.id) && !isManagedModelProvider(group.provider),
    );
  }, [chatGroups, modelList]);

  const geminiSelectedLabel = React.useMemo(() => {
    if (!current_model?.use_model) return '';
    const liveProvider =
      enabledGroups.find((group) => group.provider.id === current_model.id)?.provider ?? current_model;
    return formatModelLabelForProvider(liveProvider, current_model.use_model);
  }, [current_model, enabledGroups]);

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
    const hasModels = enabledGroups.length > 0;

    // Per-model health dot color.
    const healthDotColor = (providerId: string, modelName: string): string | null => {
      const matchedProvider = modelConfig?.find((p) => p.id === providerId);
      const healthStatus = modelHealthOf(matchedProvider, modelName)?.status || 'unknown';
      if (healthStatus === 'unknown') return null;
      return healthStatus === 'healthy' ? 'bg-green-500' : healthStatus === 'unhealthy' ? 'bg-red-500' : 'bg-gray-400';
    };

    // Mirror the ACP selector exactly: the droplist is the bare <Menu> (no wrapper
    // box, no forced min-width), so Arco's native popup styling keeps it as smooth
    // as the ACP agent dropdown.
    return (
      <Dropdown
        trigger='click'
        droplist={
          <Menu selectedKeys={current_model ? [current_model.id + current_model.use_model] : []}>
            {!hasModels
              ? [
                  <Menu.Item
                    key='no-models'
                    className='px-12px py-12px text-t-secondary text-14px text-center flex justify-center items-center'
                    disabled
                  >
                    {t('settings.noAvailableModels')}
                  </Menu.Item>,
                ]
              : [
                  ...(defaultModelUnavailable
                    ? [
                        <Menu.Item key='unavailable-default' disabled className='text-12px text-t-secondary'>
                          {t('conversation.chat.defaultModelUnavailable')}
                        </Menu.Item>,
                      ]
                    : []),
                  ...enabledGroups.map(({ provider, models }) => {
                    return (
                      <Menu.ItemGroup title={providerLabel(provider)} key={provider.id}>
                        {models.map((modelName) => {
                          const dot = healthDotColor(provider.id, modelName);
                          return (
                            <Menu.Item
                              key={compositeKey(provider.id, modelName)}
                              className={
                                current_model?.id === provider.id && current_model?.use_model === modelName
                                  ? '!bg-2'
                                  : ''
                              }
                              onClick={() => {
                                setCurrentModel({ ...provider, use_model: modelName }).catch((error) => {
                                  console.error('Failed to set current model:', error);
                                });
                              }}
                            >
                              <div className='flex items-center justify-between gap-12px w-full min-w-0'>
                                <div className='flex items-center gap-8px min-w-0'>
                                  {dot && <div className={`w-6px h-6px rounded-full shrink-0 ${dot}`} />}
                                  <span className='truncate min-w-0'>
                                    {formatModelLabelForProvider(provider, modelName)}
                                  </span>
                                </div>
                                <ModelCreditRateHint provider={provider} modelName={modelName} />
                              </div>
                            </Menu.Item>
                          );
                        })}
                      </Menu.ItemGroup>
                    );
                  }),
                ]}
          </Menu>
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
            <Menu selectedKeys={selectedAcpModel ? [selectedAcpModel] : []}>
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
                    onClick={() => setSelectedAcpModel(model.id)}
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
