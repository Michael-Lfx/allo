
import React, { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Select } from '@arco-design/web-react';
import {
  VISUAL_STYLE_PRESETS,
  promptForVisualStyleKey,
  visualStyleSelectValue,
} from '../visualStylePresets';

interface VisualStyleSelectProps {
  value: string;
  onChange: (stylePrompt: string) => void;
  disabled?: boolean;
}

const VisualStyleSelect: React.FC<VisualStyleSelectProps> = ({ value, onChange, disabled }) => {
  const { t } = useTranslation();
  const selectValue = visualStyleSelectValue(value);
  const trimmed = value.trim();

  const options = useMemo(() => {
    const presetOptions = VISUAL_STYLE_PRESETS.map((preset) => ({
      value: preset.key,
      label: t(preset.labelKey, { defaultValue: preset.defaultLabel }),
    }));
    if (selectValue === '__custom__' && trimmed) {
      return [
        ...presetOptions,
        {
          value: '__custom__',
          label: t('videoGeneration.workspace.source.stylePresets.custom', {
            defaultValue: '自定义风格',
          }),
        },
      ];
    }
    return presetOptions;
  }, [selectValue, t, trimmed]);

  return (
    <Select
      value={selectValue}
      disabled={disabled}
      options={options}
      placeholder={t('videoGeneration.workspace.source.stylePlaceholder', {
        defaultValue: '选择视觉风格',
      })}
      getPopupContainer={() => document.body}
      triggerProps={{
        autoAlignPopupWidth: true,
      }}
      onChange={(key) => {
        if (typeof key !== 'string' || key === '__custom__') return;
        onChange(promptForVisualStyleKey(key));
      }}
    />
  );
};

export default VisualStyleSelect;
