/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Dropdown } from '@arco-design/web-react';
import { Brain, Down } from '@icon-park/react';
import { configService } from '@/common/config/configService';
import { useConfig } from '@/renderer/hooks/config/useConfig';
import { iconColors } from '@/renderer/styles/colors';
import { useModelsForTask } from '@/renderer/hooks/agent/useModelsForTask';
import type { ProviderId } from '@/common/types/ids';
import { useModelSelectorProviderLabel } from '@/renderer/hooks/agent/useModelSelectorProviderLabel';
import ChatModelPickerMenu from '@/renderer/components/model/ChatModelPickerMenu';
import {
  AUTO_TIER_LABEL_FALLBACK,
  buildChatModelPickerViewModel,
  findChatModelOption,
  resolvePersistedModelChoice,
  type AutoTier,
} from '@/renderer/utils/model/chatModelPicker';

/**
 * A picked provider+model pair for every learning AI call (reflection
 * grading, course generation, job retry), or `null` if no model has been
 * resolved yet (e.g. while catalog is loading).
 */
export type LearningModelChoice = { provider_id: ProviderId; model: string } | null;

const STORAGE_KEY = 'learning.autogenModel';

/**
 * Persisted-default selection for the learning AI calls. Reads/writes
 * `learning.autogenModel`. A stored choice is honored even before the catalog
 * settles; only an authoritative catalog may replace an unavailable one with
 * the default model. `choice` is null while no concrete model can be resolved,
 * so callers must block model-less requests.
 */
export function useLearningAutogenModel() {
  const [stored] = useConfig(STORAGE_KEY);
  const { groups, isLoading, error, refresh } = useModelsForTask('chat');

  const modelPicker = useMemo(() => buildChatModelPickerViewModel(groups), [groups]);

  // `isLoading` also covers the failed state (SWR clears data on error), so an
  // error keeps the catalog unresolved and the persisted choice stays trusted.
  const catalogResolved = !isLoading && !error;
  const choice = useMemo<LearningModelChoice>(
    () => resolvePersistedModelChoice(modelPicker, stored, catalogResolved) as LearningModelChoice,
    [stored?.provider_id, stored?.model, modelPicker, catalogResolved]
  );

  const setChoice = useCallback(async (next: LearningModelChoice) => {
    if (next) {
      await configService.set(STORAGE_KEY, { provider_id: next.provider_id, model: next.model });
    } else {
      await configService.remove(STORAGE_KEY);
    }
  }, []);

  return { choice, setChoice, isLoading, error, refresh };
}

type LearningModelSelectorProps = {
  choice: LearningModelChoice;
  onChange: (choice: LearningModelChoice) => void;
  /** Match the surrounding AI buttons (mini in the form label, small in headers). */
  size?: 'mini' | 'small';
  disabled?: boolean;
};

/**
 * Compact provider+model dropdown for the learning AI calls using ChatModelPickerMenu.
 * Automatically defaults to Auto Model or first available catalog model.
 */
const LearningModelSelector: React.FC<LearningModelSelectorProps> = ({
  choice,
  onChange,
  size = 'mini',
  disabled,
}) => {
  const { t } = useTranslation();
  const { groups, isLoading, error: catalogError, refresh: refreshCatalog } = useModelsForTask('chat');
  const providerLabel = useModelSelectorProviderLabel();
  const modelPicker = useMemo(() => buildChatModelPickerViewModel(groups), [groups]);

  const selectedOption = useMemo(() => {
    if (!choice) return undefined;
    return findChatModelOption(modelPicker, choice.provider_id, choice.model);
  }, [choice, modelPicker]);

  const choiceAvailable = !choice || Boolean(selectedOption);
  const choiceUnavailable = Boolean(choice && !isLoading && !choiceAvailable);

  const autoTierLabel = (tier?: AutoTier) =>
    tier
      ? t(`conversation.modelPicker.autoTier.${tier}`, {
          defaultValue: AUTO_TIER_LABEL_FALLBACK[tier],
        })
      : t('conversation.modelPicker.autoTier.unknown', { defaultValue: 'Auto' });

  const selectedLabel = selectedOption
    ? selectedOption.family === 'auto'
      ? `${t('conversation.modelPicker.auto', { defaultValue: 'Auto' })} · ${autoTierLabel(selectedOption.autoTier)}`
      : selectedOption.label
    : choice?.model || '';

  const buttonLabel = choice
    ? choiceUnavailable
      ? `${selectedLabel || choice.model} · ${t('learning.form.modelUnavailable')}`
      : selectedLabel
    : isLoading
      ? t('common.loading')
      : t('conversation.welcome.selectModel', { defaultValue: '选择模型' });

  return (
    <Dropdown
      trigger='click'
      getPopupContainer={() => document.body}
      droplist={
        <ChatModelPickerMenu
          viewModel={modelPicker}
          selectedOption={selectedOption}
          isLoading={isLoading}
          catalogError={catalogError}
          onSelect={(option) => onChange({ provider_id: option.provider.id, model: option.model })}
          onRetry={refreshCatalog}
          providerLabel={providerLabel}
        />
      }
      disabled={disabled}
    >
      <Button
        size={size}
        type='text'
        disabled={disabled}
        status={choiceUnavailable ? 'warning' : undefined}
        title={
          choiceUnavailable
            ? t('learning.form.modelUnavailableHint')
            : t('learning.form.modelSelectTooltip')
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

export default LearningModelSelector;
