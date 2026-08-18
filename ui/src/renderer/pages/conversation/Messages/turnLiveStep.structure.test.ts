/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const messageListSource = readFileSync(new URL('./MessageList.tsx', import.meta.url), 'utf8');
const cssSource = readFileSync(new URL('./messages.css', import.meta.url), 'utf8');
const zhMessages = JSON.parse(
  readFileSync(new URL('../../../services/i18n/locales/zh-CN/messages.json', import.meta.url), 'utf8')
) as Record<string, Record<string, string> | string>;
const enMessages = JSON.parse(
  readFileSync(new URL('../../../services/i18n/locales/en-US/messages.json', import.meta.url), 'utf8')
) as Record<string, Record<string, string> | string>;

describe('turn live step strip', () => {
  test('appends the live step to the display list on both return paths', () => {
    expect(messageListSource.includes("import { planTurnLiveStep } from './turnLiveStepModel'")).toBe(true);
    expect(messageListSource.includes('const liveStepForDisclosures = buildTurnLiveStep(disclosureItems)')).toBe(true);
    expect(messageListSource.includes('const liveStep = buildTurnLiveStep(withDeliverables)')).toBe(true);
    expect(messageListSource.includes("data-testid='turn-live-step'")).toBe(true);
  });

  test('renders through the existing receipt row without detail expansion', () => {
    expect(messageListSource.includes("type: 'turn_live_step'")).toBe(true);
    expect(messageListSource.includes('hasDetail: false')).toBe(true);
  });

  test('does not add a breathing pulse on top of live-step state', () => {
    expect(cssSource.includes('@keyframes turn-live-step-breathing')).toBe(false);
    expect(cssSource.includes('turn-live-step-breathing')).toBe(false);
  });

  test('keeps the live step in document flow so page scrolling is unchanged', () => {
    expect(cssSource.includes('.turn_live_step')).toBe(true);
    expect(cssSource.includes('position: sticky')).toBe(false);
    expect(cssSource.includes('position: fixed')).toBe(false);
    expect(messageListSource.includes("className='turn-live-step'")).toBe(true);
    expect(messageListSource.includes('position: sticky')).toBe(false);
    expect(messageListSource.includes('position: fixed')).toBe(false);
  });

  test('keeps every live-step kind in document flow without a viewport follow anchor', () => {
    expect(messageListSource.includes('data-scroll-follow-anchor')).toBe(false);
    expect(messageListSource.includes("data-testid='turn-live-step'")).toBe(true);
    expect(messageListSource.includes("plan.kind === 'composing'")).toBe(true);
    expect(messageListSource.includes("plan.kind === 'analyzing'")).toBe(true);
  });

  test('reserves a stable last-line height so status swaps do not jitter the strip', () => {
    const liveStepRule = cssSource.slice(
      cssSource.indexOf('.turn_live_step {'),
      cssSource.indexOf('}', cssSource.indexOf('.turn_live_step {'))
    );
    expect(liveStepRule.includes('display: none')).toBe(false);
    expect(liveStepRule.includes('min-height: 26px')).toBe(true);
  });

  test('ships bilingual live-step copy', () => {
    expect((zhMessages.turnLiveStep as Record<string, string>).analyzing).toBe('正在分析需求');
    expect((zhMessages.turnLiveStep as Record<string, string>).composing).toBe('正在整理回复');
    expect((enMessages.turnLiveStep as Record<string, string>).analyzing).toBeTruthy();
    expect((enMessages.turnLiveStep as Record<string, string>).composing).toBeTruthy();
  });
});
