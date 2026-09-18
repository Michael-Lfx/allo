import React, { useEffect, useMemo, useRef } from 'react';
import { Select } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import { formatCloudModelLabel } from '@/renderer/utils/model/cloudModelLabel';
import { useGeneratorModels } from '@renderer/pages/workshop/generation/useGeneratorModels';
import { pickDefaultLlmModel } from '../components/modelPreferenceDefaults';
import type { GenerationPreferences } from './types';

type SelectProps = React.ComponentProps<typeof Select>;

interface PreferencesPlanningModelSelectProps {
  value: GenerationPreferences;
  disabled?: boolean;
  selectProps: Pick<SelectProps, 'getPopupContainer' | 'dropdownMenuStyle' | 'dropdownMenuClassName'> & {
    triggerProps?: SelectProps['triggerProps'];
  };
  onChange: (next: GenerationPreferences) => void;
}

const PreferencesPlanningModelSelect: React.FC<PreferencesPlanningModelSelectProps> = ({
  value,
  disabled,
  selectProps,
  onChange,
}) => {
  const { t } = useTranslation();
  const llmModels = useGeneratorModels('text', { enabled: true });
  const valueRef = useRef(value);
  valueRef.current = value;

  const llmOptions = useMemo(() => {
    const seen = new Set<string>();
    const opts: { label: string; value: string }[] = [];
    for (const model of llmModels.flat) {
      if (seen.has(model.model)) continue;
      seen.add(model.model);
      opts.push({
        value: model.model,
        label: `${formatCloudModelLabel(model.model)} · ${model.providerName}`,
      });
    }
    return opts;
  }, [llmModels.flat]);

  const safeLlmValue = llmOptions.some((option) => option.value === value.models.llm_model)
    ? value.models.llm_model
    : undefined;

  useEffect(() => {
    if (llmOptions.length === 0) return;
    const current = valueRef.current;
    const next =
      !current.models.llm_model ||
      !llmOptions.some((option) => option.value === current.models.llm_model)
        ? pickDefaultLlmModel(llmOptions.map((option) => option.value)) ?? llmOptions[0].value
        : null;
    if (!next || next === current.models.llm_model) return;
    onChange({ ...current, models: { ...current.models, llm_model: next } });
  }, [llmOptions, onChange, value.models.llm_model]);

  return (
    <Select
      allowClear={false}
      disabled={disabled || value.automatic}
      placeholder={t('videoGeneration.workspace.models.llmPlaceholder', {
        defaultValue: '选择聊天模型',
      })}
      value={safeLlmValue}
      options={llmOptions}
      notFoundContent={t('videoGeneration.workspace.models.empty', {
        defaultValue: '暂无可用模型',
      })}
      onChange={(next) =>
        onChange({
          ...value,
          models: { ...value.models, llm_model: String(next ?? '') },
        })
      }
      {...selectProps}
      triggerProps={{
        ...selectProps.triggerProps,
        position: 'tl',
      }}
    />
  );
};

export default PreferencesPlanningModelSelect;
