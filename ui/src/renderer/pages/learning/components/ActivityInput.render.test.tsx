/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { createInstance } from 'i18next';
import React from 'react';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { renderToStaticMarkup } from 'react-dom/server';

import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';
import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import { ActivityInput } from './ActivityInput';

const initTestI18n = async (locale: 'zh-CN' | 'en-US') => {
  const instance = createInstance();
  await instance.use(initReactI18next).init({
    lng: locale,
    fallbackLng: 'zh-CN',
    resources: {
      'zh-CN': { translation: { learning: learningZh } },
      'en-US': { translation: { learning: learningEn } },
    },
    interpolation: { escapeValue: false },
  });
  return instance;
};

const renderInput = async (element: React.ReactElement, locale: 'zh-CN' | 'en-US' = 'zh-CN') =>
  renderToStaticMarkup(<I18nextProvider i18n={await initTestI18n(locale)}>{element}</I18nextProvider>);

const noop = () => {};

const countOccurrences = (haystack: string, needle: string): number =>
  haystack.split(needle).length - 1;

describe('题面作答面渲染管线（n1 页面集成断言）', () => {
  // 真机报障同款场景：无理数选择题选项 $…$ 原样露出——选项必须出 KaTeX。
  test('单选选项行内公式出 KaTeX，判分 value 保持原始选项串', async () => {
    const markup = await renderInput(
      <ActivityInput
        kind='single_choice'
        options={['$0.375$', '$0.303003\\ldots$']}
        value={undefined}
        onChange={noop}
      />
    );
    expect(countOccurrences(markup, 'class="katex"')).toBeGreaterThanOrEqual(2);
    expect(countOccurrences(markup, 'data-testid="inline-markdown"')).toBe(2);
    expect(markup).toContain('arco-radio');
    // 判分锚点不受渲染影响：Radio value 仍是原始选项串
    expect(markup).toContain('$0.375$');
  });

  test('多选选项同样走渲染管线', async () => {
    const markup = await renderInput(
      <ActivityInput
        kind='multi_choice'
        options={['$\\sqrt{2}$ 是无理数', '$0.5$ 是有理数']}
        value={[]}
        onChange={noop}
      />
    );
    expect(countOccurrences(markup, 'class="katex"')).toBeGreaterThanOrEqual(2);
    expect(countOccurrences(markup, 'data-testid="inline-markdown"')).toBe(2);
    expect(markup).toContain('arco-checkbox');
  });

  test('排序条目走渲染管线', async () => {
    const markup = await renderInput(
      <ActivityInput
        kind='ordering'
        options={['$\\sqrt{2}$', '$\\pi$']}
        value={undefined}
        onChange={noop}
      />
    );
    expect(countOccurrences(markup, 'data-testid="inline-markdown"')).toBe(2);
    expect(countOccurrences(markup, 'class="katex"')).toBeGreaterThanOrEqual(2);
  });

  test('英文主题下同一断言成立（选项文案来自数据面，与 locale 无关）', async () => {
    const markup = await renderInput(
      <ActivityInput kind='single_choice' options={['$\\pi$']} value={undefined} onChange={noop} />,
      'en-US'
    );
    expect(countOccurrences(markup, 'class="katex"')).toBe(1);
  });

  test('fill_in_blank 作答位仍是输入框（空位渲染只发生在题干，不在作答控件）', async () => {
    const markup = await renderInput(
      <ActivityInput kind='fill_in_blank' options={[]} value={''} onChange={noop} />
    );
    expect(markup).toContain('arco-input');
    expect(markup).not.toContain('data-blank-slot');
  });

  test('matching 行布局（l1）：label 40% 真实基准 + select 60% 有界——label 不再塌缩 80px 窄柱', async () => {
    const markup = await renderInput(
      <ActivityInput
        kind='matching'
        options={['执行「新创建一个对象」后得到的对象']}
        matches={['堆区']}
        value={undefined}
        onChange={noop}
      />
    );
    // label 列 40% 真实基准（原 flex-1 = 基准 0，被 Arco Select width:100%
    // 的 basis 吸收全部收缩量 → 塌缩到 min-width 80px 竖排长短语）。
    expect(markup).toContain('flex-[1_1_40%]');
    // select 60% 基准 + min-w 有界。
    expect(markup).toContain('flex-[1_1_60%]');
    expect(markup).toContain('min-w-140px');
  });

  test('classify 行布局（pf2）：对象行 + 类别 select，l1 布局与 matching 同款', async () => {
    const markup = await renderInput(
      <ActivityInput
        kind='classify'
        options={['对象一', '对象二']}
        matches={['堆区', '栈区']}
        value={undefined}
        onChange={noop}
      />
    );
    // 每个对象一行 select（两个对象行）。
    expect(markup).toContain('对象一');
    expect(markup).toContain('对象二');
    expect(countOccurrences(markup, 'arco-select')).toBeGreaterThanOrEqual(2);
    // l1 布局同款。
    expect(markup).toContain('flex-[1_1_40%]');
    expect(markup).toContain('flex-[1_1_60%]');
    expect(markup).toContain('min-w-140px');
    // placeholder 走 i18n（类别候选在弹出层渲染，静态 markup 不可见）。
    // 双 locale 断言（red-line §2.3）：zh 与 en 各钉一条。
    expect(markup).toContain('选择类别');
    const markupEn = await renderInput(
      <ActivityInput
        kind='classify'
        options={['对象一', '对象二']}
        matches={['堆区', '栈区']}
        value={undefined}
        onChange={noop}
      />,
      'en-US'
    );
    expect(markupEn).toContain('Pick a category');
  });
});

describe('提交后逐项判分标记（pf1）', () => {
  // 语义 token（danger/success 族）双主题适配；类串逐字面落源码供 dead-css 提取。
  const MARKED_SUCCESS = 'border-[var(--color-success-light-3)] bg-[var(--color-success-light-1)]';
  const MARKED_DANGER = 'border-[var(--color-danger-light-3)] bg-[var(--color-danger-light-1)]';

  test('matching 逐行：错行红、对行绿（本波动机源头：哪行错必须可见）', async () => {
    const markup = await renderInput(
      <ActivityInput
        kind='matching'
        options={['左一', '左二', '左三']}
        matches={['右甲']}
        value={['右甲', '右甲', '右甲']}
        itemResults={[false, true, true]}
        disabled
        onChange={noop}
      />
    );
    expect(countOccurrences(markup, MARKED_DANGER)).toBe(1);
    expect(countOccurrences(markup, MARKED_SUCCESS)).toBe(2);
  });

  test('classify 逐对象行：归错类红、归对类绿（pf2 判别题接 pf1 管道）', async () => {
    const markup = await renderInput(
      <ActivityInput
        kind='classify'
        options={['对象一', '对象二', '对象三']}
        matches={['堆区', '栈区']}
        value={['堆区', '堆区', '堆区']}
        itemResults={[false, true, true]}
        disabled
        onChange={noop}
      />
    );
    expect(countOccurrences(markup, MARKED_DANGER)).toBe(1);
    expect(countOccurrences(markup, MARKED_SUCCESS)).toBe(2);
  });

  test('classify 降级：无 itemResults（旧 attempt / 旧后端）不染色', async () => {
    const markup = await renderInput(
      <ActivityInput
        kind='classify'
        options={['对象一', '对象二']}
        matches={['堆区', '栈区']}
        value={['堆区', '堆区']}
        disabled
        onChange={noop}
      />
    );
    expect(markup).not.toContain(MARKED_DANGER);
    expect(markup).not.toContain(MARKED_SUCCESS);
  });

  test('ordering 逐位：错位次红、对位绿', async () => {
    const markup = await renderInput(
      <ActivityInput
        kind='ordering'
        options={['甲', '乙', '丙']}
        value={['甲', '丙', '乙']}
        itemResults={[true, false, false]}
        disabled
        onChange={noop}
      />
    );
    expect(countOccurrences(markup, MARKED_DANGER)).toBe(2);
    expect(countOccurrences(markup, MARKED_SUCCESS)).toBe(1);
  });

  test('single_choice：错选标红，未选项不染色', async () => {
    const markup = await renderInput(
      <ActivityInput
        kind='single_choice'
        options={['甲', '乙']}
        value='甲'
        itemResults={[false]}
        disabled
        onChange={noop}
      />
    );
    expect(countOccurrences(markup, MARKED_DANGER)).toBe(1);
    expect(countOccurrences(markup, MARKED_SUCCESS)).toBe(0);
  });

  test('single_choice：选对标绿', async () => {
    const markup = await renderInput(
      <ActivityInput
        kind='single_choice'
        options={['甲', '乙']}
        value='乙'
        itemResults={[true]}
        disabled
        onChange={noop}
      />
    );
    expect(countOccurrences(markup, MARKED_SUCCESS)).toBe(1);
    expect(countOccurrences(markup, MARKED_DANGER)).toBe(0);
  });

  test('true_false：错选标红', async () => {
    const markup = await renderInput(
      <ActivityInput
        kind='true_false'
        options={[]}
        value={true}
        itemResults={[false]}
        disabled
        onChange={noop}
      />
    );
    expect(countOccurrences(markup, MARKED_DANGER)).toBe(1);
    expect(countOccurrences(markup, MARKED_SUCCESS)).toBe(0);
  });

  test('multi_choice：整体对错标色（picked 随单元素结果统一染色）', async () => {
    const markup = await renderInput(
      <ActivityInput
        kind='multi_choice'
        options={['甲', '乙', '丙']}
        value={['甲', '乙']}
        itemResults={[false]}
        disabled
        onChange={noop}
      />
    );
    expect(countOccurrences(markup, MARKED_DANGER)).toBe(2);
    expect(countOccurrences(markup, MARKED_SUCCESS)).toBe(0);
  });

  test('fill_in_blank：错值红', async () => {
    const markup = await renderInput(
      <ActivityInput
        kind='fill_in_blank'
        options={[]}
        value='堆区'
        itemResults={[false]}
        disabled
        onChange={noop}
      />
    );
    expect(markup).toContain(MARKED_DANGER);
    expect(markup).not.toContain(MARKED_SUCCESS);
  });

  test('numeric：对值绿', async () => {
    const markup = await renderInput(
      <ActivityInput
        kind='numeric'
        options={[]}
        value={42}
        itemResults={[true]}
        disabled
        onChange={noop}
      />
    );
    expect(markup).toContain(MARKED_SUCCESS);
    expect(markup).not.toContain(MARKED_DANGER);
  });

  test('reflection / open_question：registry 门关闭，给了 itemResults 也不染色', async () => {
    for (const kind of ['reflection', 'open_question'] as const) {
      const markup = await renderInput(
        <ActivityInput
          kind={kind}
          options={[]}
          value='学员的回答'
          itemResults={[false]}
          disabled
          onChange={noop}
        />
      );
      expect(markup).not.toContain(MARKED_DANGER);
      expect(markup).not.toContain(MARKED_SUCCESS);
    }
  });

  test('降级：无 itemResults（旧 attempt / 旧后端）保持全局文案，不染色', async () => {
    const markup = await renderInput(
      <ActivityInput
        kind='matching'
        options={['左一', '左二']}
        matches={['右甲']}
        value={['右甲', '右甲']}
        disabled
        onChange={noop}
      />
    );
    expect(markup).not.toContain(MARKED_DANGER);
    expect(markup).not.toContain(MARKED_SUCCESS);
  });
});
