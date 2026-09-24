/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, it } from 'bun:test';
import {
  buildPresetExtractionDraft,
  buildPresetInstructions,
  derivePresetName,
  recommendSkillIds,
} from './extractPresetModel';

describe('extractPresetModel', () => {
  it('derives a friendly preset name from user question or conversation title', () => {
    expect(derivePresetName({ messageText: 'ok', userQuestion: '帮我重构 React 组件' })).toBe(
      '重构 React 组件助手'
    );
    expect(derivePresetName({ messageText: 'ok', conversationTitle: 'Python 数据分析' })).toBe(
      'Python 数据分析助手'
    );
    expect(derivePresetName({ messageText: 'ok' })).toBe('智能专属助手');
  });

  it('builds structured system instructions with optional user guidance', () => {
    const promptWithoutGuidance = buildPresetInstructions({
      messageText: '这是一个方案',
      userQuestion: '设计架构',
    });
    expect(promptWithoutGuidance).toContain('# 角色定位');
    expect(promptWithoutGuidance).toContain('# 工作流程与原则');
    expect(promptWithoutGuidance).toContain('# 规范与偏好');
    expect(promptWithoutGuidance).not.toContain('用户定制补充要求');

    const promptWithGuidance = buildPresetInstructions({
      messageText: '这是一个方案',
      userQuestion: '设计架构',
      userGuidance: '输出严格使用 TypeScript 并加上详尽注释',
    });
    expect(promptWithGuidance).toContain('用户定制补充要求');
    expect(promptWithGuidance).toContain('输出严格使用 TypeScript 并加上详尽注释');
  });

  it('recommends relevant skill IDs based on content keywords and supports skill_id format', () => {
    const availableSkills = [
      { skill_id: 'builtin:code_edit', name: 'Code Editor' },
      { skill_id: 'builtin:web_search', name: 'Web Search' },
      { skill_id: 'builtin:browser', name: 'Browser Automation' },
    ];

    const codeSkills = recommendSkillIds('帮我写一个代码重构脚本', availableSkills);
    expect(codeSkills).toContain('builtin:code_edit');

    const searchSkills = recommendSkillIds('请在网上搜索最新技术新闻', availableSkills);
    expect(searchSkills).toContain('builtin:web_search');
  });

  it('builds a complete draft ready for preset editor drawer', () => {
    const draft = buildPresetExtractionDraft({
      conversationTitle: '前端优化',
      userQuestion: '请帮我优化首屏加载性能',
      messageText: '这里是首屏加载优化步骤与代码：...',
      userGuidance: '严格遵循 React 19 最佳实践',
      availableSkills: [{ id: 'builtin:code_edit', name: 'Code Edit' }],
    });

    expect(draft.name).toBe('优化首屏加载性能助手');
    expect(draft.description).toContain('React 19');
    expect(draft.context).toContain('# 角色定位');
    expect(draft.context).toContain('严格遵循 React 19 最佳实践');
    expect(draft.skills).toEqual(['builtin:code_edit']);
    expect(draft.avatar).toBe('🤖');
  });
});
