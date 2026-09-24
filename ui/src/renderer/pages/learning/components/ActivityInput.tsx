import { Button, Checkbox, Input, InputNumber, Radio, Select } from '@arco-design/web-react';
import { IconArrowDown, IconArrowUp } from '@arco-design/web-react/icon';
import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ActivityKind } from '../types';

/** 排序题：条目上下移动产出学习者的顺序（value = 条目数组） */
function OrderingInput({
  options,
  value,
  disabled,
  onChange,
}: {
  options: string[];
  value: unknown;
  disabled?: boolean;
  onChange: (value: unknown) => void;
}) {
  const sequence: string[] = useMemo(() => {
    const picked = Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : [];
    // 从 options 出发应用学习者已确定的顺序，未排过的条目保持展示顺序
    const remaining = options.filter((option) => !picked.includes(option));
    return [...picked, ...remaining];
  }, [options, value]);
  const move = (index: number, delta: -1 | 1) => {
    const target = index + delta;
    if (target < 0 || target >= sequence.length) return;
    const next = [...sequence];
    [next[index], next[target]] = [next[target], next[index]];
    // 完整序列才视为有效作答（与后端顺序判定一致）
    onChange(next.length === options.length ? next : next);
  };
  return (
    <div className='flex flex-col gap-4px'>
      {sequence.map((item, index) => (
        <div
          key={item}
          className='flex items-center gap-6px rounded-6px border border-solid border-[var(--color-border-2)] px-8px py-4px'
        >
          <span className='text-12px text-t-tertiary'>{index + 1}.</span>
          <span className='flex-1'>{item}</span>
          <Button
            size='mini'
            icon={<IconArrowUp />}
            disabled={disabled || index === 0}
            onClick={() => move(index, -1)}
          />
          <Button
            size='mini'
            icon={<IconArrowDown />}
            disabled={disabled || index === sequence.length - 1}
            onClick={() => move(index, 1)}
          />
        </div>
      ))}
    </div>
  );
}

/** 匹配题：左列每个条目选一个右列候选（value = 与 options 对齐的数组）；
 * 右列候选在本地打乱，避免展示顺序泄漏答案对齐 */
function MatchingInput({
  options,
  matches,
  value,
  disabled,
  onChange,
}: {
  options: string[];
  matches: string[];
  value: unknown;
  disabled?: boolean;
  onChange: (value: unknown) => void;
}) {
  const { t } = useTranslation();
  const shuffled = useMemo(() => {
    const base = matches.length > 0 ? matches : options;
    const copy = [...base];
    for (let index = copy.length - 1; index > 0; index -= 1) {
      const swap = Math.floor(Math.random() * (index + 1));
      [copy[index], copy[swap]] = [copy[swap], copy[index]];
    }
    return copy;
  }, [matches, options]);
  const alignment: (string | undefined)[] = Array.isArray(value)
    ? value.map((item) => (typeof item === 'string' ? item : undefined))
    : [];
  const pick = (index: number, candidate: string | undefined) => {
    const next = [...alignment];
    while (next.length < options.length) next.push(undefined);
    next[index] = candidate;
    onChange(next);
  };
  return (
    <div className='flex flex-col gap-6px'>
      {options.map((left, index) => (
        <div key={left} className='flex items-center gap-8px'>
          <span className='min-w-80px flex-1'>{left}</span>
          <Select
            className='min-w-140px'
            disabled={disabled}
            allowClear
            placeholder={t('learning.matchingPickPlaceholder')}
            value={alignment[index]}
            onChange={(next) => pick(index, next)}
          >
            {shuffled.map((candidate) => (
              <Select.Option key={candidate} value={candidate}>
                {candidate}
              </Select.Option>
            ))}
          </Select>
        </div>
      ))}
    </div>
  );
}

/** 题型作答输入：覆盖全部九种题型。
 * 复习卡片与课时活动共用，保证同一种题型在两处交互一致。 */
export function ActivityInput({
  kind,
  options,
  matches = [],
  value,
  disabled,
  onChange,
  placeholder,
  direction = 'vertical',
}: {
  kind: ActivityKind;
  options: string[];
  /** matching 题的右列候选；其余题型忽略 */
  matches?: string[];
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
  if (kind === 'multi_choice') {
    const picked = Array.isArray(value)
      ? value.filter((item): item is string => typeof item === 'string')
      : [];
    return (
      <Checkbox.Group
        direction={direction}
        disabled={disabled}
        value={picked}
        onChange={(next: string[]) => onChange(next)}
      >
        {options.map((option) => (
          <Checkbox key={option} value={option}>
            {option}
          </Checkbox>
        ))}
      </Checkbox.Group>
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
  if (kind === 'fill_in_blank') {
    return (
      <Input
        value={typeof value === 'string' ? value : ''}
        placeholder={placeholder ?? t('learning.fillBlankPlaceholder')}
        disabled={disabled}
        onChange={onChange}
      />
    );
  }
  if (kind === 'numeric') {
    return (
      <InputNumber
        value={typeof value === 'number' ? value : undefined}
        placeholder={placeholder ?? t('learning.numericPlaceholder')}
        disabled={disabled}
        onChange={(next) => onChange(typeof next === 'number' ? next : undefined)}
      />
    );
  }
  if (kind === 'ordering') {
    return (
      <OrderingInput options={options} value={value} disabled={disabled} onChange={onChange} />
    );
  }
  if (kind === 'matching') {
    return (
      <MatchingInput
        options={options}
        matches={matches}
        value={value}
        disabled={disabled}
        onChange={onChange}
      />
    );
  }
  // reflection 与 open_question 同为 AI 批改的开放作答
  return (
    <Input.TextArea
      value={typeof value === 'string' ? value : ''}
      placeholder={placeholder ?? t('learning.reflectionPlaceholder')}
      autoSize={{ minRows: 3, maxRows: 8 }}
      onChange={onChange}
    />
  );
}
