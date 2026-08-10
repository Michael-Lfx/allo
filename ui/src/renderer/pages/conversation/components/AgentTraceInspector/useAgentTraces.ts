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
  artifact_count?: number;
  input_tokens: number;
  output_tokens: number;
  stop_reason?: string | null;
  success?: boolean | null;
  relative_path: string;
}

/** Metadata-only verified artifact (no absolute path / no bytes). */
export interface AgentTraceArtifactMeta {
  id: string;
  kind: string;
  mime_type: string;
  relative_path: string;
  size_bytes: number;
  sha256: string;
  call_id?: string | null;
  tool_name?: string | null;
  /** `receipt` = verified PersistedArtifact; `reported` = Write/Edit-style path. */
  source?: string | null;
}

export interface AgentTraceArtifactIndexEntry {
  schema_version: number;
  trace_id: string;
  conversation_id: string;
  msg_id: string;
  started_at_ms: number;
  artifact: AgentTraceArtifactMeta;
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
  artifact_count?: number;
  artifacts?: AgentTraceArtifactMeta[];
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

export async function listAgentTraceArtifacts(
  conversationId: string,
  limit = 100
): Promise<AgentTraceArtifactIndexEntry[]> {
  const params = new URLSearchParams({
    conversation_id: conversationId,
    limit: String(limit),
  });
  return httpRequest<AgentTraceArtifactIndexEntry[]>(
    'GET',
    `/api/debug/agent-traces/artifacts?${params.toString()}`,
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

/** Parse span.attributes.artifacts when present. */
export function spanArtifacts(
  attributes: Record<string, unknown> | undefined
): AgentTraceArtifactMeta[] {
  const raw = attributes?.artifacts;
  if (!Array.isArray(raw)) return [];
  const out: AgentTraceArtifactMeta[] = [];
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue;
    const row = item as Record<string, unknown>;
    const id = typeof row.id === 'string' ? row.id : null;
    const relative_path = typeof row.relative_path === 'string' ? row.relative_path : null;
    if (!id || !relative_path) continue;
    out.push({
      id,
      kind: typeof row.kind === 'string' ? row.kind : 'file',
      mime_type: typeof row.mime_type === 'string' ? row.mime_type : '',
      relative_path,
      size_bytes: typeof row.size_bytes === 'number' ? row.size_bytes : 0,
      sha256: typeof row.sha256 === 'string' ? row.sha256 : '',
      call_id: typeof row.call_id === 'string' ? row.call_id : null,
      tool_name: typeof row.tool_name === 'string' ? row.tool_name : null,
      source: typeof row.source === 'string' ? row.source : null,
    });
  }
  return out;
}
