/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export interface PresetDraftData {
  name: string;
  description: string;
  context: string;
  skills: string[];
  avatar: string;
}

export interface ExtractPresetSkillItem {
  skill_id?: string;
  id?: string;
  name?: string;
  description?: string;
}

export interface ExtractPresetInput {
  conversationTitle?: string;
  messageText?: string;
  userQuestion?: string;
  userGuidance?: string;
  availableSkills?: ExtractPresetSkillItem[];
}

/**
 * Derives a concise preset name from conversation context and user question.
 */
export function derivePresetName(input: ExtractPresetInput): string {
  const candidate = (input.userQuestion || input.conversationTitle || '').trim();
  if (!candidate) {
    return '智能专属助手';
  }

  // Clean leading prefixes
  const cleaned = candidate
    .replace(/^(请问|帮我|请帮我|如何|怎么|我想|怎样|能不能)/i, '')
    .replace(/[？?。！!]+$/, '')
    .trim();

  if (cleaned.length <= 16 && cleaned.length >= 2) {
    return `${cleaned}助手`;
  }

  if (cleaned.length > 16) {
    return `${cleaned.slice(0, 14)}助手`;
  }

  return '智能专属助手';
}

/**
 * Builds structured system instructions for the preset.
 */
export function buildPresetInstructions(input: ExtractPresetInput): string {
  const roleName = derivePresetName(input);
  const sections: string[] = [];

  sections.push(`# 角色定位\n你是一名经验丰富、严谨专业的${roleName}。你擅长结合上下文，提供结构清晰、深度定制的高质量解决方案。`);

  sections.push(
    `# 工作流程与原则\n1. 仔细阅读并理解用户提出的目标与背景信息。\n2. 结构化拆解问题，提供针对性、模块化的分析与具体执行步骤。\n3. 注重实践有效性与安全性，给出必要的边界说明与注意事项。`
  );

  const constraints: string[] = [];
  constraints.push('- 输出结构分明，善用小标题与列表提升可读性。');
  constraints.push('- 涉及代码或技术方案时，遵循行业最佳实践与现代规范。');

  if (input.userGuidance && input.userGuidance.trim().length > 0) {
    constraints.push(`- **用户定制补充要求**：${input.userGuidance.trim()}`);
  }

  sections.push(`# 规范与偏好\n${constraints.join('\n')}`);

  return sections.join('\n\n');
}

/**
 * Recommends relevant skill IDs based on text content and available skills.
 */
export function recommendSkillIds(
  messageContent?: string,
  availableSkills: ExtractPresetSkillItem[] = []
): string[] {
  if (!Array.isArray(availableSkills) || availableSkills.length === 0) return [];
  const lower = (messageContent || '').toLowerCase();
  const matched = new Set<string>();

  for (const skill of availableSkills) {
    if (!skill) continue;
    const rawId = skill.skill_id || skill.id || '';
    if (!rawId) continue;
    const skillName = (skill.name || rawId).toLowerCase();
    const id = rawId.toLowerCase();
    const desc = (skill.description || '').toLowerCase();

    if (
      (lower.includes('code') || lower.includes('代码') || lower.includes('重构')) &&
      (id.includes('code') || id.includes('file') || skillName.includes('code') || desc.includes('code'))
    ) {
      matched.add(rawId);
    }
    if (
      (lower.includes('search') || lower.includes('搜索') || lower.includes('查一下')) &&
      (id.includes('search') || skillName.includes('search') || desc.includes('search'))
    ) {
      matched.add(rawId);
    }
    if (
      (lower.includes('web') || lower.includes('网页') || lower.includes('浏览器')) &&
      (id.includes('browser') || id.includes('web') || skillName.includes('browser') || desc.includes('web'))
    ) {
      matched.add(rawId);
    }
    if (
      (lower.includes('terminal') || lower.includes('bash') || lower.includes('命令行') || lower.includes('终端')) &&
      (id.includes('bash') || id.includes('terminal') || skillName.includes('bash') || desc.includes('terminal'))
    ) {
      matched.add(rawId);
    }
    if (
      (lower.includes('pdf') || lower.includes('文档') || lower.includes('表格') || lower.includes('excel')) &&
      (id.includes('pdf') || id.includes('office') || id.includes('doc') || skillName.includes('pdf') || desc.includes('pdf'))
    ) {
      matched.add(rawId);
    }
    if (
      (lower.includes('git') || lower.includes('commit') || lower.includes('分支')) &&
      (id.includes('git') || skillName.includes('git') || desc.includes('git'))
    ) {
      matched.add(rawId);
    }
  }

  return Array.from(matched).slice(0, 4);
}

/**
 * Builds a complete preset draft from conversation and user guidance.
 */
export function buildPresetExtractionDraft(input: ExtractPresetInput): PresetDraftData {
  const name = derivePresetName(input);
  const instructions = buildPresetInstructions(input);
  const combinedText = `${input.userQuestion || ''}\n${input.messageText || ''}\n${input.userGuidance || ''}`;
  const skills = recommendSkillIds(combinedText, input.availableSkills);

  const description = input.userGuidance?.trim()
    ? `专注于${name.replace(/助手$/, '')}领域，且满足：${input.userGuidance.trim()}`
    : `专注于${name.replace(/助手$/, '')}领域的专业专属助手，提供高标准执行与规范输出。`;

  return {
    name,
    description: description.slice(0, 120),
    context: instructions,
    skills,
    avatar: '🤖',
  };
}
