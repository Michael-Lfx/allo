
import React, { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Select } from '@arco-design/web-react';
import {
  VISUAL_STYLE_CATEGORIES,
  presetsInCategory,
  promptForVisualStyleKey,
  visualStyleSelectValue,
} from '../visualStylePresets';

interface VisualStyleSelectProps {
  value: string;
  onChange: (stylePrompt: string) => void;
  disabled?: boolean;
}

const OptGroup = Select.OptGroup;
const Option = Select.Option;

const VisualStyleSelect: React.FC<VisualStyleSelectProps> = ({ value, onChange, disabled }) => {
  const { t } = useTranslation();
  const selectValue = visualStyleSelectValue(value);
  const trimmed = value.trim();

  const customLabel = useMemo(
    () =>
      t('videoGeneration.workspace.source.stylePresets.custom', {
        defaultValue: '自定义风格',
      }),
    [t]
  );

  return (
    <Select
      value={selectValue}
      disabled={disabled}
      placeholder={t('videoGeneration.workspace.source.stylePlaceholder', {
        defaultValue: '选择视觉风格',
      })}
      getPopupContainer={() => document.body}
      showSearch
      filterOption={(inputValue, option) => {
        const label = String(option?.props?.children ?? '').toLowerCase();
        return label.includes(String(inputValue).toLowerCase());
      }}
      triggerProps={{
        autoAlignPopupWidth: true,
      }}
      onChange={(key) => {
        if (typeof key !== 'string' || key === '__custom__') return;
        onChange(promptForVisualStyleKey(key));
      }}
    >
      {VISUAL_STYLE_CATEGORIES.map((category) => {
        const presets = presetsInCategory(category.id);
        if (presets.length === 0) return null;
        return (
          <OptGroup
            key={category.id}
            label={t(category.labelKey, { defaultValue: category.defaultLabel })}
          >
            {presets.map((preset) => (
              <Option key={preset.key} value={preset.key}>
                {t(preset.labelKey, { defaultValue: preset.defaultLabel })}
              </Option>
            ))}
          </OptGroup>
        );
      })}
      {selectValue === '__custom__' && trimmed ? (
        <OptGroup
          key='custom'
          label={t('videoGeneration.workspace.source.styleCategories.custom', {
            defaultValue: '自定义',
          })}
        >
          <Option value='__custom__'>{customLabel}</Option>
        </OptGroup>
      ) : null}
    </Select>
  );
};

export default VisualStyleSelect;
