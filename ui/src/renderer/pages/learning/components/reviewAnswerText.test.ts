/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';

import type { ActivityKind } from '../types';
import { answerText } from './reviewAnswerText';

const translate = (key: string) => (key === 'learning.trueLabel' ? '真' : '假');

describe('复习正确答案渲染（x03）', () => {
  const cases: Array<{ kind: ActivityKind; answer: unknown; options: string[]; expected: string }> = [
    { kind: 'single_choice', answer: '0.375', options: ['0.375', '0.303'], expected: '0.375' },
    { kind: 'true_false', answer: true, options: [], expected: '真' },
    { kind: 'fill_in_blank', answer: ['方向', 'direction'], options: [], expected: '方向 / direction' },
    { kind: 'multi_choice', answer: ['A', 'C'], options: ['A', 'B', 'C'], expected: 'A / C' },
    { kind: 'numeric', answer: 3.14, options: [], expected: '3.14' },
    { kind: 'ordering', answer: ['第二', '第一'], options: ['第一', '第二'], expected: '1. 第二；2. 第一' },
    { kind: 'matching', answer: ['B', 'A'], options: ['左一', '左二'], expected: '左一 → B；左二 → A' },
  ];

  for (const { kind, answer, options, expected } of cases) {
    test(`${kind} 的正确答案渲染为非空文本`, () => {
      const text = answerText(answer, kind, options, translate);
      expect(text).toBe(expected);
      expect(text.length).toBeGreaterThan(0);
    });
  }

  // AI 批改题的答案契约就是 null（后端 validate_shape），揭示区整行不渲染；
  // 这里钉住空值不崩、不产出伪文本。
  test('reflection / open_question 的 null 答案不渲染', () => {
    expect(answerText(null, 'reflection', [], translate)).toBe('');
    expect(answerText(null, 'open_question', [], translate)).toBe('');
  });

  test('未知形状走 JSON 兜底，不吞成空串', () => {
    expect(answerText({ 左一: 'B' }, 'matching', [], translate)).toBe('左一 → B');
    expect(answerText([1, 2], 'numeric', [], translate)).toBe('1 / 2');
    expect(answerText({ nested: [1, 'x'] }, 'single_choice', [], translate)).toBe('nested → 1 / x');
  });
});
