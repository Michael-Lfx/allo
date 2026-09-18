/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { parseConversationId } from '@/common/types/ids';
import {
  recalledNomiUsage,
  rememberNomiUsage,
  tokenUsageFromMetricsPayload,
} from './nomiUsageGauge';

const CONVERSATION_A = parseConversationId('0190f5fe-7c00-7a00-8000-0000000000aa');
const CONVERSATION_B = parseConversationId('0190f5fe-7c00-7a00-8000-0000000000bb');

describe('tokenUsageFromMetricsPayload', () => {
  test('maps occupancy plus session totals without inventing tokens', () => {
    expect(
      tokenUsageFromMetricsPayload({
        elapsed_ms: 1500,
        input_tokens: 120,
        output_tokens: 40,
        cache_read_tokens: 80,
        context_tokens: 1800,
        context_window: 200000,
      })
    ).toEqual({
      total_tokens: 160,
      input_tokens: 120,
      output_tokens: 40,
      cache_read_tokens: 80,
      elapsed_ms: 1500,
      context_tokens: 1800,
      context_window: 200000,
      context_breakdown: undefined,
      moa: null,
    });
  });

  test('rejects non-objects', () => {
    expect(tokenUsageFromMetricsPayload(null)).toBeNull();
    expect(tokenUsageFromMetricsPayload('usage_updated')).toBeNull();
  });
});

describe('rememberNomiUsage', () => {
  test('recalls the latest snapshot for a conversation', () => {
    rememberNomiUsage(CONVERSATION_A, { total_tokens: 10, context_tokens: 100, context_window: 1000 });
    rememberNomiUsage(CONVERSATION_B, { total_tokens: 20, context_tokens: 200, context_window: 2000 });
    rememberNomiUsage(CONVERSATION_A, { total_tokens: 30, context_tokens: 300, context_window: 1000 });

    expect(recalledNomiUsage(CONVERSATION_A)?.total_tokens).toBe(30);
    expect(recalledNomiUsage(CONVERSATION_B)?.context_tokens).toBe(200);
  });
});
