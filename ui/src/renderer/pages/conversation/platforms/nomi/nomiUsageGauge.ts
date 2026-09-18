/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { TokenUsageData } from '@/common/config/storage';
import type { MoaTurnStatsData } from '@/common/protocolBindings/MoaTurnStatsData';
import type { ConversationId } from '@/common/types/ids';

const MAX_CACHED_SESSIONS = 32;
const liveUsageByConversation = new Map<string, TokenUsageData>();

const validTokenCount = (value: unknown): number | undefined =>
  typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : undefined;

export function rememberNomiUsage(conversationId: ConversationId, usage: TokenUsageData): void {
  const key = String(conversationId);
  if (liveUsageByConversation.has(key)) {
    liveUsageByConversation.delete(key);
  }
  liveUsageByConversation.set(key, usage);
  if (liveUsageByConversation.size <= MAX_CACHED_SESSIONS) {
    return;
  }
  const oldest = liveUsageByConversation.keys().next().value;
  if (oldest) {
    liveUsageByConversation.delete(oldest);
  }
}

export function recalledNomiUsage(conversationId: ConversationId): TokenUsageData | null {
  return liveUsageByConversation.get(String(conversationId)) ?? null;
}

export function tokenUsageFromMetricsPayload(metrics: unknown): TokenUsageData | null {
  if (!metrics || typeof metrics !== 'object') {
    return null;
  }
  const payload = metrics as {
    elapsed_ms?: number;
    input_tokens?: number;
    output_tokens?: number;
    reasoning_tokens?: number;
    cache_creation_tokens?: number;
    cache_read_tokens?: number;
    context_tokens?: number;
    context_window?: number;
    context_breakdown?: TokenUsageData['context_breakdown'];
    moa?: MoaTurnStatsData | null;
  };
  const inputTokens = validTokenCount(payload.input_tokens);
  const outputTokens = validTokenCount(payload.output_tokens);
  const reasoningTokens = validTokenCount(payload.reasoning_tokens);
  const cacheCreationTokens = validTokenCount(payload.cache_creation_tokens);
  const cacheReadTokens = validTokenCount(payload.cache_read_tokens);
  return {
    total_tokens: (inputTokens ?? 0) + (outputTokens ?? 0),
    ...(inputTokens !== undefined ? { input_tokens: inputTokens } : {}),
    ...(outputTokens !== undefined ? { output_tokens: outputTokens } : {}),
    ...(reasoningTokens !== undefined ? { reasoning_tokens: reasoningTokens } : {}),
    ...(cacheCreationTokens !== undefined ? { cache_creation_tokens: cacheCreationTokens } : {}),
    ...(cacheReadTokens !== undefined ? { cache_read_tokens: cacheReadTokens } : {}),
    ...(typeof payload.elapsed_ms === 'number' && Number.isFinite(payload.elapsed_ms)
      ? { elapsed_ms: payload.elapsed_ms }
      : {}),
    context_tokens: validTokenCount(payload.context_tokens),
    context_window: validTokenCount(payload.context_window),
    context_breakdown: payload.context_breakdown,
    moa: payload.moa ?? null,
  };
}
