/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 来源覆盖卡被排除/重复文件行渲染（o4 B3）：分诊 role 枚举走词典映射
 * （core/practice/… 不再以机器名上屏），未知角色回「其他内容」；文件路径
 * 与理由是诊断原文，如实保留。中英文案都从字典取。 */
import { describe, expect, test } from 'bun:test';
import { createInstance } from 'i18next';
import React from 'react';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { renderToStaticMarkup } from 'react-dom/server';

import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';
import type { CourseSourcePlanDetailView } from '../types';
import { SourceExcludedLines } from './CourseSourceCoverageCard';

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

const detail: CourseSourcePlanDetailView = {
  excluded: [
    { path: 'notes/core.md', role: 'core', reason: '与目标强相关' },
    { path: 'notes/warmup.md', role: 'prerequisite', reason: '前置铺垫' },
    { path: 'notes/drills.md', role: 'practice', reason: '练习材料' },
    { path: 'notes/half.md', role: 'mixed', reason: '部分相关' },
    { path: 'notes/scratch.md', role: 'noise', reason: '与目标无关' },
    { path: 'notes/copy.md', role: 'duplicate', reason: '与另一文件重复' },
    { path: 'notes/maybe.md', role: 'uncertain', reason: '分诊不确定' },
    { path: 'notes/future.md', role: 'role_not_yet_known', reason: '未知角色' },
  ],
  duplicates: [
    { path: 'notes/copy.md', duplicate_of: 'notes/origin.md', canonical_reason: '内容更完整' },
  ],
};

const renderLines = async (locale: 'zh-CN' | 'en-US'): Promise<string> => {
  const instance = await initTestI18n(locale);
  return renderToStaticMarkup(
    <I18nextProvider i18n={instance}>
      <SourceExcludedLines detail={detail} />
    </I18nextProvider>
  );
};

describe('SourceExcludedLines render (o4 B3)', () => {
  test('role 枚举映射为中文角色名，机器名不再上屏（zh）', async () => {
    const html = await renderLines('zh-CN');
    expect(html).toContain('核心资料');
    expect(html).toContain('前置资料');
    expect(html).toContain('练习资料');
    expect(html).toContain('混合内容');
    expect(html).toContain('无关内容');
    expect(html).toContain('重复内容');
    expect(html).toContain('存疑内容');
    // 未知角色回「其他内容」，不落机器名。
    expect(html).toContain('其他内容');
    expect(html).not.toContain('[core]');
    expect(html).not.toContain('[noise]');
    expect(html).not.toContain('[uncertain]');
    expect(html).not.toContain('[duplicate]');
    expect(html).not.toContain('role_not_yet_known');
    // 诊断原文（路径与理由）如实保留。
    expect(html).toContain('notes/core.md');
    expect(html).toContain('与目标强相关');
  });

  test('en 同口径：role 走英文名，路径理由保留', async () => {
    const html = await renderLines('en-US');
    expect(html).toContain('Core material');
    expect(html).toContain('Unrelated content');
    expect(html).toContain('Other content');
    expect(html).not.toContain('[core]');
    expect(html).toContain('notes/scratch.md');
  });

  test('重复行：role 名 + canonical 指向与理由保留', async () => {
    const html = await renderLines('zh-CN');
    expect(html).toContain('重复内容 notes/copy.md → notes/origin.md');
    expect(html).toContain('内容更完整');
  });
});
