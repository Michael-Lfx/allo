/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { Button, Dropdown, Menu } from '@arco-design/web-react';
import { Brain, Down, Plus } from '@icon-park/react';
import { configService } from '@/common/config/configService';
import { modelHealthOf } from '@/common/utils/providerModels';
import { useConfig } from '@/renderer/hooks/config/useConfig';
import { iconColors } from '@/renderer/styles/colors';
import { useModelsForTask } from '@/renderer/hooks/agent/useModelsForTask';
import type { ProviderId } from '@/common/types/ids';
import { useModelSelectorProviderLabel } from '@/renderer/hooks/agent/useModelSelectorProviderLabel';
import { formatModelLabelForProvider } from '@/renderer/utils/model/cloudModelLabel';
import ModelCreditRateHint from '@/renderer/components/model/ModelCreditRateHint';

export type EvalModelChoice = { provider_id: ProviderId; model: string } | null;

const STORAGE_KEY = 'eval.autogenModel';

export function useEvalAutogenModel() {
  const [stored] = useConfig(STORAGE_KEY);

  const choice = useMemo<EvalModelChoice>(() => {
    if (!stored?.provider_id || !stored.model) return null;
    return { provider_id: stored.provider_id, model: stored.model };
  }, [stored?.provider_id, stored?.model]);

  const setChoice = useCallback(async (next: EvalModelChoice) => {
    if (next) {
      await configService.set(STORAGE_KEY, { provider_id: next.provider_id, model: next.model });
    } else {
      await configService.remove(STORAGE_KEY);
    }
  }, []);

  return { choice, setChoice };
}

type EvalModelSelectorProps = {
  choice: EvalModelChoice;
  onChange: (choice: EvalModelChoice) => void;
  size?: 'mini' | 'small';
  disabled?: boolean;
};

const EvalModelSelector: React.FC<EvalModelSelectorProps> = ({
  choice,
  onChange,
  size = 'mini',
  disabled,
}) => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { groups, isLoading } = useModelsForTask('chat');
  const providerLabel = useModelSelectorProviderLabel();

  const defaultLabel = t('common.defaultModel');
  const choiceAvailable =
    !choice ||
    groups.some(
      (group) =>
        group.provider.id === choice.provider_id && group.models.includes(choice.model)
    );
  const choiceUnavailable = Boolean(choice && !isLoading && !choiceAvailable);
  const selectedProvider = choice
    ? groups.find((group) => group.provider.id === choice.provider_id)?.provider
    : undefined;
  const selectedLabel = choice
    ? formatModelLabelForProvider(selectedProvider, choice.model)
    : '';
  const buttonLabel = choice
    ? choiceUnavailable
      ? `${selectedLabel || choice.model} · ${t('eval.form.modelUnavailable')}`
      : selectedLabel
    : defaultLabel;

  const droplist = (
    <Menu selectedKeys={choice ? [`${choice.provider_id}:${choice.model}`] : ['__default__']}>
      <Menu.Item key='__default__' onClick={() => onChange(null)}>
        {defaultLabel}
      </Menu.Item>
      {groups.length === 0
        ? [
            <Menu.Item
              key='add-model'
              className='text-12px text-t-secondary'
              onClick={() => navigate('/models?section=models')}
            >
              <Plus theme='outline' size='12' />
              {t('settings.addModel')}
            </Menu.Item>,
          ]
        : groups.map(({ provider, models }) => (
            <Menu.ItemGroup title={providerLabel(provider)} key={provider.id}>
              {models.map((modelName) => {
                const healthStatus = modelHealthOf(provider, modelName)?.status || 'unknown';
                const healthColor =
                  healthStatus === 'healthy'
                    ? 'bg-green-500'
                    : healthStatus === 'unhealthy'
                      ? 'bg-red-500'
                      : 'bg-gray-400';
                return (
                  <Menu.Item
                    key={`${provider.id}:${modelName}`}
                    onClick={() => onChange({ provider_id: provider.id, model: modelName })}
                  >
                    <div className='flex items-center justify-between gap-12px w-full min-w-0'>
                      <div className='flex items-center gap-8px min-w-0'>
                        {healthStatus !== 'unknown' && (
                          <div className={`w-6px h-6px rounded-full shrink-0 ${healthColor}`} />
                        )}
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
          ))}
    </Menu>
  );

  return (
    <Dropdown trigger='click' droplist={droplist} disabled={disabled}>
      <Button
        size={size}
        type='text'
        disabled={disabled}
        status={choiceUnavailable ? 'warning' : undefined}
        title={
          choiceUnavailable
            ? t('eval.form.modelUnavailableHint')
            : t('eval.form.modelSelectTooltip')
        }
      >
        <span className='flex items-center gap-4px min-w-0 max-w-160px'>
          <Brain theme='outline' size='12' fill={iconColors.secondary} className='shrink-0' />
          <span className='truncate'>{buttonLabel}</span>
          <Down theme='outline' size='10' fill={iconColors.secondary} className='shrink-0' />
        </span>
      </Button>
    </Dropdown>
  );
};

export default EvalModelSelector;
