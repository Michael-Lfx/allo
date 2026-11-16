/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 口语化波（learning-plain-language o4）词典契约：结构区术语双语改写 +
 * 机器直出点新增映射词典。断言直接读真实词典 JSON（红线 2：双语词典真实
 * 存在且对称，不是 zh 照抄 en），目标病词（主干/门口/投影/端点/宁滥勿缺/
 * ISO 704/剔除/连贯预算/实时计算…）在本票辖区逐 key 归零。 */
import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';

const zh = learningZh as unknown as Record<string, string>;
const en = learningEn as unknown as Record<string, string>;

/** 双语取词：缺词即红（本票 key 两语都必须在场且非空） */
function pair(key: string): { zh: string; en: string } {
  const zhValue = zh[key];
  const enValue = en[key];
  if (typeof zhValue !== 'string' || zhValue.length === 0) {
    throw new Error(`zh-CN/learning.json 缺 key「${key}」`);
  }
  if (typeof enValue !== 'string' || enValue.length === 0) {
    throw new Error(`en-US/learning.json 缺 key「${key}」`);
  }
  return { zh: zhValue, en: enValue };
}

/** A 组：结构区术语口语化（票面逐 key 双语改写方向） */
describe('plain language dictionary (o4 group A)', () => {
  test('StructureSummary/StructCounts：主干/块/门口 → 主线/知识组/入门检查', () => {
    const summary = pair('learningGraphStructureSummary');
    expect(summary.zh).toBe('{{spines}} 条主线 · {{blocks}} 个知识组 · {{doors}} 个入门检查');
    expect(summary.en).toBe(
      '{{spines}} main lines · {{blocks}} knowledge groups · {{doors}} entry checks'
    );
    const counts = pair('learningGraphStructCounts');
    expect(counts.zh).toBe('{{blocks}} 个知识组 · {{spines}} 条主线');
    expect(counts.en).toBe('{{blocks}} knowledge groups · {{spines}} main lines');
  });

  test('RouteSpine 族：主干 → 主线；自动补前导 → 自动补齐前置', () => {
    const spine = pair('learningGraphStructRouteSpine');
    expect(spine.zh).toBe('主线：{{name}}（{{count}} 个知识组，自动补齐前置）');
    expect(spine.en).toBe('Main line: {{name}} ({{count}} knowledge groups, prerequisites auto-filled)');
    expect(spine.en).not.toBe(spine.zh);

    const all = pair('learningGraphStructRouteAll');
    expect(all.zh).toBe('全景（全部知识组）');
    expect(all.en).toBe('Overview (all knowledge groups)');

    const hide = pair('learningGraphStructHideSpines');
    expect(hide.zh).toBe('隐藏主线');
    expect(hide.en).toBe('Hide main lines');

    const matrix = pair('learningGraphStructMatrixSpine');
    expect(matrix.zh).toBe('主线');
    expect(matrix.en).toBe('Main line');
  });

  test('Relation_assumes：前置（门）→ 前置知识；DoorRefsLabel：门口覆盖 → 对应的入门检查', () => {
    const assumes = pair('learningGraphStructRelation_assumes');
    expect(assumes.zh).toBe('前置知识');
    expect(assumes.en).toBe('Prerequisite');
    expect(assumes.zh).not.toContain('门');

    const doorRefs = pair('learningGraphStructDoorRefsLabel');
    expect(doorRefs.zh).toBe('对应的入门检查');
    expect(doorRefs.en).toBe('Related entry checks');
    expect(doorRefs.zh).not.toContain('门口');
  });

  test('CreateHint/StructureHint：参考地图隐喻保留，主干与知识块 → 主线与章节', () => {
    for (const key of ['learningGraphCreateHint', 'learningGraphStructureHint']) {
      const value = pair(key);
      expect(value.zh).toContain('主线与章节');
      expect(value.zh).toContain('参考地图');
      expect(value.zh).not.toMatch(/主干|知识块/);
      expect(value.en).toContain('main lines and chapters');
      expect(value.en).not.toMatch(/spine/i);
    }
  });

  test('StructFooter：投影非权威 → 此图仅供了解结构', () => {
    const footer = pair('learningGraphStructFooter');
    expect(footer.zh).toBe('此图仅供了解结构；层级表示先后依赖，不代表进度。');
    expect(footer.en).toBe('This graph is for orientation only; layers show dependency order, not progress.');
    expect(footer.zh).not.toContain('投影');
  });

  test('StructUnregistered：未收录端点 → 未归入结构的概念；StructLayer：L{n} → 第 N 层', () => {
    const unregistered = pair('learningGraphStructUnregistered');
    expect(unregistered.zh).toBe('{{count}} 个未归入结构的概念');
    expect(unregistered.en).toBe('{{count}} concepts not placed in the structure');
    expect(unregistered.zh).not.toContain('端点');

    const layer = pair('learningGraphStructLayer');
    expect(layer.zh).toBe('第 {{layer}} 层（越靠前越基础）');
    expect(layer.en).toBe('Layer {{layer}} (earlier = more foundational)');
    expect(layer.zh).not.toMatch(/依赖层|L\{\{/);
  });

  test('ConceptsEmpty：教与假定 → 教到的与默认你已会的', () => {
    const value = pair('learningGraphConceptsEmpty');
    expect(value.zh).toContain('这里会列出它教到的、以及默认你已会的概念');
    expect(value.en).toContain('what it teaches and what it assumes you already know');
  });

  test('Audit 三 Tooltip：宁滥勿缺/ISO 704/循环定义 → 人话', () => {
    const fragments = pair('learningGraphAuditFragmentsTooltip');
    expect(fragments.zh).toBe('定义中未被词表解释的词：{{fragments}}');
    expect(fragments.en).toBe('Words in the definition that the lexicon does not explain: {{fragments}}');
    expect(fragments.zh).not.toMatch(/宁滥勿缺|切词噪声|用语表/);
    expect(fragments.en).not.toMatch(/over-inclusive|tokenization/i);

    const selfRef = pair('learningGraphAuditSelfRefTooltip');
    expect(selfRef.zh).toBe('定义里出现了这个词自己');
    expect(selfRef.en).toBe('The definition uses the term itself');
    expect(selfRef.zh).not.toMatch(/ISO|正名/);
    expect(selfRef.en).not.toContain('ISO 704');

    const mutual = pair('learningGraphAuditMutualTooltip');
    expect(mutual.zh).toBe('这个词和「{{name}}」互相解释，等于没解释');
    expect(mutual.en).toBe('This term and "{{name}}" explain each other, which explains nothing');
    expect(mutual.zh).not.toContain('循环定义');
  });

  test('conceptRelationsHint：删「实时计算，不单独存储」开发注记', () => {
    const value = pair('conceptRelationsHint');
    expect(value.zh).toBe('关系根据概念定义中的相互引用自动整理；点击概念可跳转。');
    expect(value.en).toBe(
      'Relations are compiled automatically from how concept definitions reference each other; click a concept to open it.'
    );
    expect(value.zh).not.toMatch(/实时计算|单独存储/);
    expect(value.en).not.toMatch(/computed live|not stored/i);
  });

  test('samplingTooltip 族：连贯预算/章节主干 → 保持章节完整', () => {
    const tooltip = pair('samplingTooltip');
    expect(tooltip.zh).toBe('优先选与目标相关的文件，并保持章节完整；以下低分文件未入样：');
    expect(tooltip.en).toBe(
      'Files are picked by relevance to the goal, keeping chapters intact; these low-score files were not sampled:'
    );
    const empty = pair('samplingTooltipEmpty');
    expect(empty.zh).toBe('优先选与目标相关的文件，并保持章节完整；本次没有低分文件未入样。');
    expect(empty.en).toBe(
      'Files are picked by relevance to the goal, keeping chapters intact; no low-score file was skipped this time.'
    );
    for (const value of [tooltip, empty]) {
      expect(value.zh).not.toMatch(/连贯预算|章节主干/);
      expect(value.en).not.toMatch(/coherence budget|chapter trunk/i);
    }
  });

  test('reviewManage：剔除 → 移除（Hint 与 HintFull 同步）', () => {
    const hint = pair('reviewManageHint');
    expect(hint.zh).toBe('编辑、归档或移除，都是记忆编程的一部分');
    expect(hint.en).toBe('Edit, archive or remove: part of memory programming');
    const full = pair('reviewManageHintFull');
    expect(full.zh).toContain('移除');
    expect(full.zh).not.toContain('剔除');
  });
});

/** B 组新增映射词典：role/原因/回退文案双语在场（组件断言见各渲染测试） */
describe('plain language dictionary (o4 group B new keys)', () => {
  test('来源分诊 role 映射八档双语齐备（core/practice/…不上屏）', () => {
    const roles: ReadonlyArray<[string, string, string]> = [
      ['sourceCoverageRoleCore', '核心资料', 'Core material'],
      ['sourceCoverageRolePrerequisite', '前置资料', 'Prerequisite material'],
      ['sourceCoverageRolePractice', '练习资料', 'Practice material'],
      ['sourceCoverageRoleMixed', '混合内容', 'Mixed content'],
      ['sourceCoverageRoleNoise', '无关内容', 'Unrelated content'],
      ['sourceCoverageRoleDuplicate', '重复内容', 'Duplicate content'],
      ['sourceCoverageRoleUncertain', '存疑内容', 'Uncertain content'],
      ['sourceCoverageRoleOther', '其他内容', 'Other content'],
    ];
    for (const [key, zhText, enText] of roles) {
      const value = pair(key);
      expect(value.zh).toBe(zhText);
      expect(value.en).toBe(enText);
    }
  });

  test('库名解析失败回退：来源库已删除（不再裸 UUID 上屏）', () => {
    const value = pair('sourceLibraryDeleted');
    expect(value.zh).toBe('来源库已删除');
    expect(value.en).toBe('Source library deleted');
  });

  test('生长 Pill 主题前缀失配回退：后台生长任务（不再整句后端串）', () => {
    const value = pair('growthPillTopicFallback');
    expect(value.zh).toBe('后台生长任务');
    expect(value.en).toBe('Background growth task');
  });

  test('挂载观察计数标签 + 折叠详情头双语齐备（后端观察句不再内插上屏）', () => {
    const outside = pair('learningGraphMountObsOutside');
    expect(outside.zh).toBe('{{count}} 个节点不在结构里');
    expect(outside.en).toBe('Nodes not placed in the structure: {{count}}');
    const emptyBlock = pair('learningGraphMountObsEmpty');
    expect(emptyBlock.zh).toBe('{{count}} 个知识组还没有节点');
    expect(emptyBlock.en).toBe('Knowledge groups without nodes: {{count}}');
    const overloaded = pair('learningGraphMountObsOverloaded');
    expect(overloaded.zh).toBe('{{count}} 个知识组节点偏多');
    expect(overloaded.en).toBe('Knowledge groups with too many nodes: {{count}}');
    const other = pair('learningGraphMountObsOther');
    expect(other.zh).toBe('其他观察 ×{{count}}');
    expect(other.en).toBe('Other observations ×{{count}}');
    const details = pair('learningGraphMountObsDetails');
    expect(details.zh).toBe('观察详情');
    expect(details.en).toBe('Observation details');
    for (const value of [outside, emptyBlock, overloaded]) {
      expect(value.zh).not.toContain('{{detail}}');
      expect(value.en).not.toContain('{{detail}}');
    }
  });

  test('BasisDeclaredDoors 只报数量：id 串占位 {{doors}} 退场', () => {
    const value = pair('learningGraphBasisDeclaredDoors');
    expect(value.zh).toBe('已声明 {{doorsCount}} 项入门检查（覆盖 {{count}} 个概念）');
    expect(value.en).toBe('{{doorsCount}} entry checks declared (covering {{count}} concepts)');
    expect(value.zh).not.toContain('{{doors}}');
    expect(value.en).not.toContain('{{doors}}');
  });

  test('BasisConfirmContent 不再内插门口 id 串', () => {
    const value = pair('learningGraphBasisConfirmContent');
    expect(value.zh).not.toContain('{{doors}}');
    expect(value.en).not.toContain('{{doors}}');
  });

  test('BasisDeclareDone toast 只报数量：门口/块 id 串占位退场（同域新发现直出点）', () => {
    const value = pair('learningGraphBasisDeclareDone');
    expect(value.zh).toContain('{{doorsCount}}');
    expect(value.zh).toContain('{{blocksCount}}');
    expect(value.zh).not.toContain('{{doors}}');
    expect(value.zh).not.toContain('{{blocks}}');
    expect(value.en).not.toContain('{{doors}}');
    expect(value.en).not.toContain('{{blocks}}');
  });

  test('源面契约：声明成功 toast 不再 join 门口/块 id（LearningGraphWorkspace）', () => {
    const source = readFileSync(
      new URL('./components/LearningGraphWorkspace.tsx', import.meta.url),
      'utf8'
    );
    expect(source).not.toContain('result.doors.join');
    expect(source).not.toContain('result.blocks.join');
  });
});
