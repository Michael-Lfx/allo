/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';
import enLearning from '../../services/i18n/locales/en-US/learning.json';
import zhLearning from '../../services/i18n/locales/zh-CN/learning.json';
import { knowledgeBaseNames } from './utils';

const readSource = (url: URL): string => readFileSync(url, 'utf8');

const hookSource = readSource(new URL('./hooks/useCourseCreation.ts', import.meta.url));
const dialogSource = readSource(new URL('./components/CreateCourseDialog.tsx', import.meta.url));
const pillSource = readSource(new URL('./components/CourseGenerationPill.tsx', import.meta.url));
const progressSource = readSource(
  new URL('./components/CourseGenerationProgress.tsx', import.meta.url)
);
const workspaceSource = readSource(new URL('./components/CourseWorkspace.tsx', import.meta.url));
const lessonSource = readSource(new URL('./components/LessonStudy.tsx', import.meta.url));

/** 多知识库（t4 评审修复）的前端纪律：上限不静默截断、列表分隔符随语言、
 * 来源披露口径共享。结构断言 + 纯函数断言，不引入渲染测试。 */
describe('learning multi-source disclosure', () => {
  test('never silently truncates the source list at the cap', () => {
    expect(hookSource.includes('.slice(0, MAX_COURSE_KNOWLEDGE_BASES)')).toBe(false);
    expect(hookSource.includes('kbSelectionTooMany')).toBe(true);
    expect(dialogSource.includes('kbSelectionTooMany')).toBe(true);
    // 选择端拒绝超限的这次选择（保留上一次的合法选择），不是截断提交。
    expect(dialogSource.includes('onSelectedBaseChange(next)')).toBe(true);
  });

  test('never hardcodes the Chinese list separator in joined names', () => {
    for (const source of [pillSource, progressSource, workspaceSource]) {
      expect(source.includes("join('、')")).toBe(false);
      expect(source.includes("join(t('learning.listSeparator'))")).toBe(true);
    }
  });

  test('the separator is locale-specific in both dictionaries', () => {
    expect(zhLearning.listSeparator).toBe('、');
    expect(enLearning.listSeparator).toBe(', ');
    expect(zhLearning.kbSelectionTooMany).toContain('{{count}}');
    expect(enLearning.kbSelectionTooMany).toContain('{{count}}');
    expect(zhLearning.sourceCountHint).toContain('{{count}}');
    expect(enLearning.sourceCountHint).toContain('{{count}}');
  });

  test('lesson panel shows the primary source plus the citation count', () => {
    expect(lessonSource.includes('sourceCount={current.sources?.length ?? 1}')).toBe(true);
    expect(lessonSource.includes("t('learning.sourceCountHint'")).toBe(true);
  });

  test('the sampling disclosure line is shared, not duplicated per view', () => {
    const lineSource = readSource(new URL('./components/CourseSamplingLine.tsx', import.meta.url));
    expect(lineSource.includes('samplingLine')).toBe(true);
    expect(workspaceSource.includes('<CourseSamplingLine')).toBe(true);
    expect(progressSource.includes('<CourseSamplingLine')).toBe(true);
    // 工作在自定义样式（悬浮提示的 cursor-help）之外的采样行渲染只有一处。
    expect(workspaceSource.includes("t('learning.samplingLine'")).toBe(false);
    expect(progressSource.includes("t('learning.samplingLine'")).toBe(false);
  });

  test('knowledgeBaseNames resolves ids to names with an honest id fallback', () => {
    const bases = [{ knowledge_base_id: 'kb-1', name: '金融笔记' }];
    expect(knowledgeBaseNames(['kb-1', 'kb-missing'], bases)).toEqual(['金融笔记', 'kb-missing']);
    expect(knowledgeBaseNames([], bases)).toEqual([]);
  });
});
