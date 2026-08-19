/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { httpRequest } from '@/common/adapter/httpBridge';

/** Matches `Integrity` in `nomi-agent-trace` (`rename_all = "snake_case"`). */
export type ObservationIntegrity = 'complete' | 'degraded';

export type ExecutionStatus =
  | 'running'
  | 'completed'
  | 'failed'
  | 'cancelled'
  | 'interrupted'
  | 'truncated'
  | 'unknown';

export type RecorderHealthStatus =
  | 'healthy'
  | 'queue_dropped'
  | 'storage_error'
  | 'writer_disconnected';

/** Matches `ObservationScope` in `nomi-agent-trace`. */
export type ObservationScope =
  | 'session_workflow'
  | 'session_auxiliary'
  | 'process_diagnostic';

export interface RecorderHealth {
  status: RecorderHealthStatus;
  last_error?: string | null;
}

export interface ObservationSummary {
  turn_count: number;
  model_call_count: number;
  tool_count: number;
  active_duration_ms: number;
  wall_span_ms?: number | null;
  integrity: ObservationIntegrity;
  coverage: string;
  max_event_seq: number;
}

export interface SessionObservationList {
  recorder_health: RecorderHealth;
  summary: ObservationSummary;
  turns: ProjectedTurn[];
}

export interface ProjectedGap {
  event_seq: number;
  reason?: string | null;
  from_seq?: number | null;
  to_seq?: number | null;
}

export interface ProjectedTokenUsage {
  input_tokens?: number | null;
  output_tokens?: number | null;
  cache_read_tokens?: number | null;
  cache_creation_tokens?: number | null;
}

export interface ProjectedToolExecution {
  tool_call_id: string;
  name?: string | null;
  started_at_ms?: number | null;
  ended_at_ms?: number | null;
  status?: 'started' | 'completed' | 'failed' | 'cancelled';
  started?: unknown | null;
  completed?: unknown | null;
  failed?: unknown | null;
  cancelled?: unknown | null;
  argument_preview?: string | null;
}

export interface ProjectedRequestSummary {
  model?: string | null;
  has_system: boolean;
  message_count: number;
  tool_definition_count: number;
  system_omitted?: boolean;
  messages_omitted?: boolean;
  tools_omitted?: boolean;
}

export interface ProjectedResponseSummary {
  has_text: boolean;
  has_thinking: boolean;
  text_omitted?: boolean;
  thinking_omitted?: boolean;
  tool_use_count: number;
  elapsed_ms?: number | null;
  ttft_ms?: number | null;
  stop_reason?: string | null;
  text_preview?: string | null;
}

export interface ProjectedModelCall {
  model_call_id: string;
  call_kind?: string | null;
  observation_scope?: ObservationScope | null;
  status?: ExecutionStatus;
  integrity?: ObservationIntegrity;
  interrupted: boolean;
  started_at_ms?: number | null;
  ended_at_ms?: number | null;
  usage?: ProjectedTokenUsage | null;
  request?: unknown | null;
  response?: unknown | null;
  request_summary?: ProjectedRequestSummary | null;
  response_summary?: ProjectedResponseSummary | null;
  tools: ProjectedToolExecution[];
}

/** Projection from `GET /api/debug/session-observations`. */
export interface ProjectedTurn {
  root_turn_id: string;
  conversation_id?: string | null;
  msg_id?: string | null;
  session_kind?: string | null;
  execution_id?: string | null;
  step_id?: string | null;
  execution_attempt_id?: string | null;
  status?: ExecutionStatus;
  integrity: ObservationIntegrity;
  interrupted: boolean;
  started_at_ms?: number | null;
  ended_at_ms?: number | null;
  elapsed_ms?: number | null;
  prompt_preview?: string | null;
  max_event_seq?: number;
  has_turn_start?: boolean;
  has_turn_end?: boolean;
  gap_count: number;
  model_calls: ProjectedModelCall[];
  gaps: ProjectedGap[];
}

export interface ObservationFetchOptions {
  signal?: AbortSignal;
}

export async function listSessionObservations(
  conversationId: string,
  options?: ObservationFetchOptions
): Promise<SessionObservationList> {
  const params = new URLSearchParams({
    conversation_id: conversationId,
    limit: '200',
  });
  return httpRequest<SessionObservationList>(
    'GET',
    `/api/debug/session-observations?${params.toString()}`,
    undefined,
    { silentStatuses: [403], signal: options?.signal }
  );
}

export async function getSessionObservationTurn(
  conversationId: string,
  rootTurnId: string,
  options?: ObservationFetchOptions
): Promise<ProjectedTurn> {
  const params = new URLSearchParams({
    conversation_id: conversationId,
  });
  return httpRequest<ProjectedTurn>(
    'GET',
    `/api/debug/session-observations/turns/${encodeURIComponent(rootTurnId)}?${params.toString()}`,
    undefined,
    { silentStatuses: [403, 404], signal: options?.signal }
  );
}

export async function getSessionObservationCall(
  conversationId: string,
  rootTurnId: string,
  modelCallId: string,
  options?: ObservationFetchOptions
): Promise<ProjectedModelCall> {
  const params = new URLSearchParams({
    conversation_id: conversationId,
  });
  return httpRequest<ProjectedModelCall>(
    'GET',
    `/api/debug/session-observations/turns/${encodeURIComponent(rootTurnId)}/calls/${encodeURIComponent(modelCallId)}?${params.toString()}`,
    undefined,
    { silentStatuses: [403, 404, 410], signal: options?.signal }
  );
}

export function isObservationRetentionError(error: unknown): boolean {
  if (!error || typeof error !== 'object') return false;
  const body = 'body' in error ? (error as { body?: unknown }).body : undefined;
  if (!body || typeof body !== 'object') return false;
  const record = body as Record<string, unknown>;
  return (
    record.reason === 'observation_retention' ||
    record.code === 'OBSERVATION_RETENTION' ||
    (record.details != null &&
      typeof record.details === 'object' &&
      (record.details as { reason?: unknown }).reason === 'observation_retention')
  );
}

export function toolStatus(
  tool: ProjectedToolExecution
): 'cancelled' | 'failed' | 'completed' | 'started' {
  if (tool.status) return tool.status;
  if (tool.cancelled != null) return 'cancelled';
  if (tool.failed != null) return 'failed';
  if (tool.completed != null) return 'completed';
  return 'started';
}

export function asRecord(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === 'object' && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return null;
}

/** Canonical request body from an `llm/request` payload. Never invent omitted fields. */
export function canonicalRequestFromPayload(payload: unknown): unknown {
  const record = asRecord(payload);
  if (!record) return payload;
  return record.request ?? payload;
}
