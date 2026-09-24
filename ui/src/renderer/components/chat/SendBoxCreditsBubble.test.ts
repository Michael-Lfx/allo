/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const bubbleSource = readFileSync(new URL('./SendBoxCreditsBubble.tsx', import.meta.url), 'utf8');
const sendBoxSource = readFileSync(new URL('./SendBox/index.tsx', import.meta.url), 'utf8');

describe('SendBoxCreditsBubble structure and integration', () => {
  test('mounts SendBoxCreditsBubble inside SendBox composer submit cluster', () => {
    expect(sendBoxSource.includes('import SendBoxCreditsBubble from')).toBe(true);
    expect(sendBoxSource.includes('<SendBoxCreditsBubble')).toBe(true);
    expect(sendBoxSource.includes('blockedTriggerCount={blockedTriggerCount}')).toBe(true);
  });

  test('blocks submission and triggers shake when credits are exhausted', () => {
    expect(sendBoxSource.includes("if (creditsState === 'exhausted')")).toBe(true);
    expect(sendBoxSource.includes('setBlockedTriggerCount(')).toBe(true);
    expect(sendBoxSource.includes('setIsShakingSend(true)')).toBe(true);
    expect(sendBoxSource.includes('credits-shake')).toBe(true);
  });

  test('renders exhausted vs low hint translations', () => {
    expect(bubbleSource.includes('conversation.sendBox.creditsExhaustedBlocked')).toBe(true);
    expect(bubbleSource.includes('conversation.sendBox.creditsLowHint')).toBe(true);
    expect(bubbleSource.includes('common.creditsBubble.exhaustedAction')).toBe(true);
    expect(bubbleSource.includes('common.creditsBubble.lowAction')).toBe(true);
    expect(bubbleSource.includes('openOfficialWebsiteCredits')).toBe(true);
  });
});
