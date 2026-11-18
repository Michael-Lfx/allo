import { Button, Checkbox, Input, InputNumber, Radio, Select } from '@arco-design/web-react';
import { IconArrowDown, IconArrowUp } from '@arco-design/web-react/icon';
import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { InlineMarkdown } from '@renderer/components/Markdown';
import { ACTIVITY_KIND_REGISTRY } from '../activityKindRegistry';
import type { ActivityKind } from '../types';

/** 逐项标记语义色（pf1）：danger/success 族 token 的 light-1 底 + light-3
 * 描边（复习卡对错结果面同款），双主题适配；类串逐字面落源码供 dead-css
 * 提取。 */
type ItemMark = 'success' | 'danger';
const MARK_SUCCESS = 'border-[var(--color-success-light-3)] bg-[var(--color-success-light-1)]';
const MARK_DANGER = 'border-[var(--color-danger-light-3)] bg-[var(--color-danger-light-1)]';

/** 第 index 项的提交后状态：registry 门（reflection/open_question 是 AI
 * 批改的整体判分语义，无逐项）+ 结果缺席/越位降级不染色（旧 attempt/
 * 旧后端兼容硬要求）。 */
const itemMarkAt = (
  kind: ActivityKind,
  itemResults: boolean[] | undefined,
  index: number
): ItemMark | undefined => {
  if (!ACTIVITY_KIND_REGISTRY[kind].perItemFeedback) return undefined;
  const correct = itemResults?.[index];
  return correct === undefined ? undefined : correct ? 'success' : 'danger';
};

/** 选项胶囊标记（choice/true_false 的选中项）。 */
const pillMarkClass = (mark: ItemMark | undefined): string | undefined =>
  mark === undefined
    ? undefined
    : `rounded-4px border border-solid px-4px ${mark === 'success' ? MARK_SUCCESS : MARK_DANGER}`;

/** 输入域标记（fill_in_blank / numeric 的作答值）。 */
const fieldMarkClass = (mark: ItemMark | undefined): string | undefined =>
  mark === undefined
    ? undefined
    : `rounded-6px border border-solid px-4px py-2px ${mark === 'success' ? MARK_SUCCESS : MARK_DANGER}`;

/** 排序题：条目上下移动产出学习者的顺序（value = 条目数组） */
function OrderingInput({
  options,
  value,
  disabled,
  itemResults,
  onChange,
}: {
  options: string[];
  value: unknown;
  disabled?: boolean;
  itemResults?: boolean[];
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
      {sequence.map((item, index) => {
        // 提交后定格（pf1）：错位次红、对位绿，逐位可见
        const mark = itemMarkAt('ordering', itemResults, index);
        return (
          <div
            key={item}
            className={`flex items-center gap-6px rounded-6px border border-solid px-8px py-4px ${mark === undefined ? 'border-[var(--color-border-2)]' : mark === 'success' ? MARK_SUCCESS : MARK_DANGER}`}
          >
            <span className='text-12px text-t-tertiary'>{index + 1}.</span>
            {/* 排序条目走内联渲染管线（题面契约允许 $…$ 与内联 markdown） */}
            <span className='flex-1'>
              <InlineMarkdown text={item} />
            </span>
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
        );
      })}
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
  itemResults,
  onChange,
}: {
  options: string[];
  matches: string[];
  value: unknown;
  disabled?: boolean;
  itemResults?: boolean[];
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
      {options.map((left, index) => {
        // 提交后定格（pf1）：行级描边——错行红（本波动机源头：哪行错
        // 必须可见）、对行绿
        const mark = itemMarkAt('matching', itemResults, index);
        return (
          <div
            key={left}
            className={
              mark === undefined
                ? 'flex items-center gap-8px'
                : `flex items-center gap-8px rounded-6px border border-solid px-8px py-2px ${mark === 'success' ? MARK_SUCCESS : MARK_DANGER}`
            }
          >
            {/* l1：label 给真实基准宽 40%（原 flex-1 = 基准 0，Arco Select 默认
                width:100% 的 basis 吸收全部收缩量 → label 塌缩到 min-width
                80px 竖排长短语）；select 60% 基准 + min-w 防极窄。 */}
            <span className='min-w-80px flex-[1_1_40%]'>{left}</span>
            <Select
              className='min-w-140px flex-[1_1_60%]'
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
        );
      })}
    </div>
  );
}

/** 题型作答输入：覆盖全部九种题型。
 * 复习卡片与课时活动共用，保证同一种题型在两处交互一致。
 * itemResults（pf1）：提交后的逐项判分结果——matching 逐行、ordering 逐
 * 位、单结果题型单元素；经题型注册表 perItemFeedback 门控（reflection/
 * open_question 不逐项），缺席 = 降级全局文案（旧 attempt 兼容）。 */
export function ActivityInput({
  kind,
  options,
  matches = [],
  value,
  disabled,
  itemResults,
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
  itemResults?: boolean[];
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
            {/* 提交后定格（pf1）：选中项按单元素结果染红/绿（错选红、
                选对绿），未选项不染色 */}
            <span
              className={pillMarkClass(
                value === option ? itemMarkAt(kind, itemResults, 0) : undefined
              )}
            >
              {/* 选项文本走内联渲染管线；value 仍为原始选项串，判分不受影响 */}
              <InlineMarkdown text={option} />
            </span>
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
            {/* 提交后定格（pf1）：整体对错标色——选中项随单元素结果统一
                染色（全对绿、漏/多选红） */}
            <span
              className={pillMarkClass(
                picked.includes(option) ? itemMarkAt(kind, itemResults, 0) : undefined
              )}
            >
              {/* 选项文本走内联渲染管线；value 仍为原始选项串，判分不受影响 */}
              <InlineMarkdown text={option} />
            </span>
          </Checkbox>
        ))}
      </Checkbox.Group>
    );
  }
  if (kind === 'true_false') {
    const pickedSide = value === undefined ? undefined : value === true ? 'true' : 'false';
    const sideMark = (side: 'true' | 'false') =>
      pillMarkClass(pickedSide === side ? itemMarkAt(kind, itemResults, 0) : undefined);
    return (
      <Radio.Group
        disabled={disabled}
        value={value === undefined ? undefined : String(value)}
        onChange={(next) => onChange(next === 'true')}
      >
        <Radio value='true'>
          <span className={sideMark('true')}>{t('learning.trueLabel')}</span>
        </Radio>
        <Radio value='false'>
          <span className={sideMark('false')}>{t('learning.falseLabel')}</span>
        </Radio>
      </Radio.Group>
    );
  }
  if (kind === 'fill_in_blank') {
    // 提交后定格（pf1）：错值红、对值绿（作答值外圈语义色域）
    return (
      <div className={fieldMarkClass(itemMarkAt(kind, itemResults, 0))}>
        <Input
          value={typeof value === 'string' ? value : ''}
          placeholder={placeholder ?? t('learning.fillBlankPlaceholder')}
          disabled={disabled}
          onChange={onChange}
        />
      </div>
    );
  }
  if (kind === 'numeric') {
    return (
      <div className={fieldMarkClass(itemMarkAt(kind, itemResults, 0))}>
        <InputNumber
          value={typeof value === 'number' ? value : undefined}
          placeholder={placeholder ?? t('learning.numericPlaceholder')}
          disabled={disabled}
          onChange={(next) => onChange(typeof next === 'number' ? next : undefined)}
        />
      </div>
    );
  }
  if (kind === 'ordering') {
    return (
      <OrderingInput
        options={options}
        value={value}
        disabled={disabled}
        itemResults={itemResults}
        onChange={onChange}
      />
    );
  }
  if (kind === 'matching') {
    return (
      <MatchingInput
        options={options}
        matches={matches}
        value={value}
        disabled={disabled}
        itemResults={itemResults}
        onChange={onChange}
      />
    );
  }
  // reflection 与 open_question 同为 AI 批改的开放作答（无逐项语义，
  // perItemFeedback 门关闭——itemResults 即使误传也不染色）
  return (
    <Input.TextArea
      value={typeof value === 'string' ? value : ''}
      placeholder={placeholder ?? t('learning.reflectionPlaceholder')}
      autoSize={{ minRows: 3, maxRows: 8 }}
      onChange={onChange}
    />
  );
}
