/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import type { MessageId } from '@/common/types/ids';
import { getActiveStreamingTextIndex, type StreamingTextCandidate } from './streamingMessageModel';

const ACTIVE_TURN = 'turn-active' as MessageId;
const REQUEST = 'request-active' as MessageId;

const text = (overrides: Partial<StreamingTextCandidate> = {}): StreamingTextCandidate => ({
  type: 'text',
  position: 'left',
  ...overrides,
});

const context = (
  overrides: Partial<Parameters<typeof getActiveStreamingTextIndex>[1]> = {}
): Parameters<typeof getActiveStreamingTextIndex>[1] => ({
  isProcessing: true,
  activeTurnId: ACTIVE_TURN,
  activeRequestMessageId: REQUEST,
  lastUserTextIndex: 0,
  ...overrides,
});

describe('getActiveStreamingTextIndex', () => {
  test('selects only the latest assistant text after the latest user message', () => {
    const items = [
      { type: 'text', position: 'right' as const },
      text({ turn_id: ACTIVE_TURN }),
      text({ turn_id: ACTIVE_TURN }),
      { type: 'turn_live_step' },
    ];

    expect(getActiveStreamingTextIndex(items, context())).toBe(2);
  });

  test('does not stream when the lifecycle is idle or has no active identity', () => {
    const items = [text({ turn_id: ACTIVE_TURN })];

    expect(getActiveStreamingTextIndex(items, context({ isProcessing: false }))).toBe(-1);
    expect(
      getActiveStreamingTextIndex(items, context({ activeTurnId: undefined, activeRequestMessageId: undefined }))
    ).toBe(-1);
  });

  test('treats a finished row as done even when the conversation is processing', () => {
    const items = [
      text({ turn_id: ACTIVE_TURN }),
      text({ turn_id: ACTIVE_TURN, status: 'finish' }),
    ];

    expect(getActiveStreamingTextIndex(items, context())).toBe(-1);
  });

  test('rejects a row belonging to another active turn', () => {
    const items = [text({ turn_id: 'turn-old' as MessageId })];

    expect(getActiveStreamingTextIndex(items, context())).toBe(-1);
  });

  test('does not use a turn-tagged row without an authoritative active turn id', () => {
    const items = [text({ turn_id: ACTIVE_TURN })];

    expect(getActiveStreamingTextIndex(items, context({ activeTurnId: undefined }))).toBe(-1);
  });

  test('falls back to the latest assistant row when legacy data lacks turn_id', () => {
    const items = [
      { type: 'text', position: 'right' as const },
      text(),
      text(),
    ];

    expect(getActiveStreamingTextIndex(items, context())).toBe(2);
  });

  test('does not consider rows before the latest user message', () => {
    const items = [text({ turn_id: ACTIVE_TURN }), { type: 'text', position: 'right' as const }];

    expect(getActiveStreamingTextIndex(items, context({ lastUserTextIndex: 1 }))).toBe(-1);
  });

  test('does not stream assistant-only history without a visible user boundary', () => {
    const items = [text({ turn_id: ACTIVE_TURN })];

    expect(getActiveStreamingTextIndex(items, context({ lastUserTextIndex: -1 }))).toBe(-1);
  });
});
