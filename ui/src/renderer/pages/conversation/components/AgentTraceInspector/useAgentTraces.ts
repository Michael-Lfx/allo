/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { httpRequest } from '@/common/adapter/httpBridge';

/** Matches `Integrity` in `nomi-agent-trace` (`rename_all = "snake_case"`). */
export type ObservationIntegrity = 'complete' | 'degraded';

/** Matches `ObservationScope` in `nomi-agent-trace`. */
export type ObservationScope =
  | 'session_workflow'
  | 'session_auxiliary'
  | 'process_diagnostic';

export interface ProjectedGap {
  event_seq: number;
  reason?: string | null;
  from_seq?: number | null;
  to_seq?: number | null;
}

export interface ProjectedToolExecution {
  tool_call_id: string;
  name?: string | null;
  started?: unknown | null;
  completed?: unknown | null;
  failed?: unknown | null;
  cancelled?: unknown | null;
}

export interface ProjectedModelCall {
  model_call_id: string;
  call_kind?: string | null;
  observation_scope?: ObservationScope | null;
  interrupted: boolean;
  request?: unknown | null;
  response?: unknown | null;
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
  integrity: ObservationIntegrity;
  interrupted: boolean;
  gap_count: number;
  model_calls: ProjectedModelCall[];
  gaps: ProjectedGap[];
}

export async function listSessionObservations(
  conversationId: string
): Promise<ProjectedTurn[]> {
  const params = new URLSearchParams({
    conversation_id: conversationId,
    limit: '200',
  });
  return httpRequest<ProjectedTurn[]>(
    'GET',
    `/api/debug/session-observations?${params.toString()}`,
    undefined,
    { silentStatuses: [403] }
  );
}

export async function getSessionObservationTurn(
  conversationId: string,
  rootTurnId: string
): Promise<ProjectedTurn> {
  const params = new URLSearchParams({
    conversation_id: conversationId,
  });
  return httpRequest<ProjectedTurn>(
    'GET',
    `/api/debug/session-observations/turns/${encodeURIComponent(rootTurnId)}?${params.toString()}`,
    undefined,
    { silentStatuses: [403, 404] }
  );
}

export function toolStatus(
  tool: ProjectedToolExecution
): 'cancelled' | 'failed' | 'completed' | 'started' {
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
