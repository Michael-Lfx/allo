/** 题型注册表（pf5）单测：九 kind descriptor 全量覆盖 + 首例数据钉住。
 * 与后端 ActivityKind capability 方法（nomifun-learning models.rs 的
 * activity_kind_registry_tests）语义对齐——两端各带同源测试。 */
import { describe, expect, test } from 'bun:test';
import { ACTIVITY_KIND_REGISTRY } from './activityKindRegistry';
import type { ActivityKind } from './types';

const ALL_KINDS: ActivityKind[] = [
  'single_choice',
  'true_false',
  'reflection',
  'fill_in_blank',
  'multi_choice',
  'numeric',
  'ordering',
  'matching',
  'open_question',
];

describe('activityKindRegistry', () => {
  test('九种题型全量覆盖：一个 kind 一条 descriptor，无缺无余', () => {
    expect(Object.keys(ACTIVITY_KIND_REGISTRY).sort()).toEqual([...ALL_KINDS].sort());
  });

  test('matching 是首例 deprecated：接替 classify、生成/门槛/抽题全关、逐项反馈保留', () => {
    expect(ACTIVITY_KIND_REGISTRY.matching).toEqual({
      status: 'deprecated',
      replacedBy: 'classify',
      llmGeneratable: false,
      countsTowardSection: false,
      reviewEligible: false,
      perItemFeedback: true,
    });
  });

  test('其余八 kind = active 全能力；reflection/open_question 无逐项反馈', () => {
    for (const kind of ALL_KINDS) {
      if (kind === 'matching') continue;
      const descriptor = ACTIVITY_KIND_REGISTRY[kind];
      expect(descriptor.status).toBe('active');
      expect(descriptor.llmGeneratable).toBe(true);
      expect(descriptor.countsTowardSection).toBe(true);
      expect(descriptor.reviewEligible).toBe(true);
      const perItem = kind !== 'reflection' && kind !== 'open_question';
      expect(descriptor.perItemFeedback).toBe(perItem);
    }
  });

  test('deprecated 条目必带接替者；active 条目不带', () => {
    for (const [kind, descriptor] of Object.entries(ACTIVITY_KIND_REGISTRY)) {
      if (descriptor.status === 'deprecated') {
        expect(descriptor.replacedBy).toBeTruthy();
      } else {
        expect(descriptor.replacedBy).toBeUndefined();
      }
    }
  });
});
