/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { httpRequest } from '@/common/adapter/httpBridge';

/** Index row from `GET /api/debug/agent-traces` (snake_case from Rust). */
export interface AgentTraceIndexEntry {
  schema_version: number;
  trace_id: string;
  conversation_id: string;
  msg_id: string;
  root_turn_id: string;
  session_kind: string;
  started_at_ms: number;
  ended_at_ms?: number | null;
  elapsed_ms?: number | null;
  tool_call_count: number;
  tool_error_count: number;
  input_tokens: number;
  output_tokens: number;
  stop_reason?: string | null;
  success?: boolean | null;
  relative_path: string;
}

export interface AgentTraceSpan {
  span_id: string;
  parent_span_id?: string | null;
  kind: string;
  name: string;
  started_at_ms: number;
  ended_at_ms?: number | null;
  status: string;
  attributes?: Record<string, unknown>;
  preview?: string | null;
}

export interface AgentTurnSummary {
  elapsed_ms?: number | null;
  input_tokens?: number;
  output_tokens?: number;
  cache_creation_tokens?: number;
  cache_read_tokens?: number;
  context_tokens?: number;
  context_window?: number;
  stop_reason?: string | null;
  tool_call_count?: number;
  tool_error_count?: number;
  llm_round_count?: number;
  success?: boolean | null;
  error_code?: string | null;
  error_message?: string | null;
}

export interface AgentTurnTrace {
  schema_version: number;
  trace_id: string;
  conversation_id: string;
  msg_id: string;
  root_turn_id: string;
  session_kind: string;
  origin?: string | null;
  companion?: boolean;
  channel_platform?: string | null;
  provider?: string | null;
  model?: string | null;
  started_at_ms: number;
  ended_at_ms?: number | null;
  spans: AgentTraceSpan[];
  summary: AgentTurnSummary;
}

export async function listAgentTraces(
  conversationId: string,
  limit = 50
): Promise<AgentTraceIndexEntry[]> {
  const params = new URLSearchParams({
    conversation_id: conversationId,
    limit: String(limit),
  });
  return httpRequest<AgentTraceIndexEntry[]>(
    'GET',
    `/api/debug/agent-traces?${params.toString()}`,
    undefined,
    { silentStatuses: [403] }
  );
}

export async function getAgentTrace(traceId: string): Promise<AgentTurnTrace> {
  return httpRequest<AgentTurnTrace>(
    'GET',
    `/api/debug/agent-traces/${encodeURIComponent(traceId)}`,
    undefined,
    { silentStatuses: [403, 404] }
  );
}
