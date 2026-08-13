import { Input, Radio } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import type { ActivityKind } from '../types';

/** 题型作答输入：单选题 / 判断题 / 反思题。
 * 复习卡片与课时活动共用，保证三种题型的交互一致。 */
export function ActivityInput({
  kind,
  options,
  value,
  disabled,
  onChange,
  placeholder,
  direction = 'vertical',
}: {
  kind: ActivityKind;
  options: string[];
  value: unknown;
  disabled?: boolean;
  onChange: (value: unknown) => void;
  placeholder?: string;
  direction?: 'vertical' | 'horizontal';
}) {
  const { t } = useTranslation();
  if (kind === 'single_choice') {
    return (
      <Radio.Group
        direction={direction}
        disabled={disabled}
        value={value as string | undefined}
        onChange={onChange}
      >
        {options.map((option) => (
          <Radio key={option} value={option}>
            {option}
          </Radio>
        ))}
      </Radio.Group>
    );
  }
  if (kind === 'true_false') {
    return (
      <Radio.Group
        disabled={disabled}
        value={value === undefined ? undefined : String(value)}
        onChange={(next) => onChange(next === 'true')}
      >
        <Radio value='true'>{t('learning.trueLabel')}</Radio>
        <Radio value='false'>{t('learning.falseLabel')}</Radio>
      </Radio.Group>
    );
  }
  return (
    <Input.TextArea
      value={typeof value === 'string' ? value : ''}
      placeholder={placeholder}
      autoSize={{ minRows: 3, maxRows: 8 }}
      onChange={onChange}
    />
  );
}
