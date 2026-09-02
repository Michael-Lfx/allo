/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import {
  BackendHttpError,
  buildBackendAuthHeaders,
  getBaseUrl,
  httpRequest,
  notifyHttpAuthFailure,
} from '@/common/adapter/httpBridge';
import { ipcBridge } from '@/common';
import { isDesktopShell } from '@renderer/utils/platform';

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

export type RequestMessageViewMode = 'current_suffix' | 'full' | 'omitted';

export interface RequestMessageView {
  mode: RequestMessageViewMode;
  hidden_message_count: number;
  visible_message_count: number;
}

export type SystemPromptState = 'first' | 'unchanged' | 'changed' | 'unavailable';

export interface ObservationTimelineEvent {
  event_seq: number;
  event_type: string;
  timestamp_ms: number;
  relative_ms: number;
  model_call_id?: string | null;
  tool_call_id?: string | null;
  call_kind?: string | null;
  tool_name?: string | null;
  status?: string | null;
  duration_ms?: number | null;
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
  request_message_view?: RequestMessageView | null;
  system_prompt_state?: SystemPromptState | null;
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
  prompt_preview_context_only?: boolean;
  max_event_seq?: number;
  has_turn_start?: boolean;
  has_turn_end?: boolean;
  gap_count: number;
  timeline: ObservationTimelineEvent[];
  model_calls: ProjectedModelCall[];
  gaps: ProjectedGap[];
}

export interface SessionObservationExportEvent {
  schema_version: number;
  event_type: string;
  event_seq: number;
  timestamp: string;
  timestamp_ms: number;
  payload: unknown;
}

export interface SessionObservationExportTurn {
  root_turn_id: string;
  conversation_id?: string | null;
  msg_id?: string | null;
  session_kind?: string | null;
  execution_id?: string | null;
  step_id?: string | null;
  execution_attempt_id?: string | null;
  status: ExecutionStatus;
  integrity: ObservationIntegrity;
  interrupted: boolean;
  started_at_ms?: number | null;
  ended_at_ms?: number | null;
  elapsed_ms?: number | null;
  prompt_preview?: string | null;
  prompt_preview_context_only?: boolean;
  max_event_seq: number;
  has_turn_start: boolean;
  has_turn_end: boolean;
  gap_count: number;
}

export interface SessionObservationExport {
  export_version: number;
  schema_version: number;
  exported_at_ms: number;
  conversation_id: string;
  root_turn_id: string;
  status: ExecutionStatus;
  integrity: ObservationIntegrity;
  coverage: string;
  has_turn_end: boolean;
  turn: SessionObservationExportTurn;
  events: SessionObservationExportEvent[];
}

export interface ObservationFetchOptions {
  signal?: AbortSignal;
}

interface BrowserSaveFilePickerOptions {
  suggestedName?: string;
  types?: Array<{
    description: string;
    accept: Record<string, string[]>;
  }>;
}

interface BrowserWritableFile {
  write(data: string): Promise<void>;
  close(): Promise<void>;
  abort?(): Promise<void>;
}

interface BrowserSaveFileHandle {
  readonly name?: string;
  createWritable(): Promise<BrowserWritableFile>;
}

type BrowserSaveFilePicker = (
  options?: BrowserSaveFilePickerOptions,
) => Promise<BrowserSaveFileHandle>;

type ObservationSaveTarget =
  | { kind: 'native'; path: string }
  | { kind: 'browser'; handle: BrowserSaveFileHandle };

/** Raised when the current WebUI browser cannot offer a user-selected path. */
export class ObservationSaveLocationError extends Error {
  readonly code = 'OBSERVATION_SAVE_LOCATION_UNAVAILABLE';

  constructor() {
    super('A user-selected save location is required to save an observation export.');
    this.name = 'ObservationSaveLocationError';
  }
}

export function isObservationSaveLocationError(
  error: unknown,
): error is ObservationSaveLocationError {
  return error instanceof ObservationSaveLocationError;
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

function fallbackObservationExportFilename(rootTurnId: string): string {
  const safeId = rootTurnId.replace(/[^a-zA-Z0-9_-]/g, '-') || 'turn';
  return `flowy-observation-${safeId}.json`;
}

function observationExportFilename(contentDisposition: string | null, rootTurnId: string): string {
  const fallback = fallbackObservationExportFilename(rootTurnId);
  if (!contentDisposition) return fallback;
  const match = contentDisposition.match(
    /filename\*=UTF-8''([^;]+)|filename="([^"]+)"|filename=([^;]+)/i
  );
  const encoded = match?.[1] ?? match?.[2] ?? match?.[3];
  if (!encoded) return fallback;
  let decoded = encoded.trim();
  try {
    decoded = decodeURIComponent(decoded);
  } catch {
    // Keep the server value when it is not percent encoded.
  }
  const safe = decoded.replace(/[\\/:*?"<>|]/g, '-').trim();
  return safe || fallback;
}

function isAbortLike(error: unknown): boolean {
  return (
    error != null &&
    typeof error === 'object' &&
    'name' in error &&
    (error as { name?: unknown }).name === 'AbortError'
  );
}

function browserSaveFilePicker(): BrowserSaveFilePicker | null {
  if (typeof window === 'undefined') return null;
  return (
    window as Window & { showSaveFilePicker?: BrowserSaveFilePicker }
  ).showSaveFilePicker ?? null;
}

async function chooseObservationSaveTarget(filename: string): Promise<ObservationSaveTarget | null> {
  if (isDesktopShell()) {
    let path: string | null;
    try {
      path = await ipcBridge.dialog.showSave.invoke({
        defaultPath: filename,
        filters: [{ name: 'JSON', extensions: ['json'] }],
      });
    } catch (error) {
      if (isAbortLike(error)) return null;
      throw error;
    }
    return path ? { kind: 'native', path } : null;
  }

  const picker = browserSaveFilePicker();
  if (!picker) throw new ObservationSaveLocationError();
  try {
    return {
      kind: 'browser',
      handle: await picker({
        suggestedName: filename,
        types: [{ description: 'JSON', accept: { 'application/json': ['.json'] } }],
      }),
    };
  } catch (error) {
    if (isAbortLike(error)) return null;
    throw error;
  }
}

function selectedFilename(path: string, fallback: string): string {
  const name = path.split(/[\\/]/).pop()?.trim();
  return name || fallback;
}

/** Save the server's retained-event export at a user-selected location. */
export async function downloadSessionObservation(
  conversationId: string,
  rootTurnId: string,
  options?: ObservationFetchOptions
): Promise<string | null> {
  // Open the picker before the fetch so WebUI browsers retain the click's
  // user-activation requirement for showSaveFilePicker().
  const target = await chooseObservationSaveTarget(fallbackObservationExportFilename(rootTurnId));
  if (!target || options?.signal?.aborted) return null;

  const params = new URLSearchParams({ conversation_id: conversationId });
  const path = `/api/debug/session-observations/turns/${encodeURIComponent(rootTurnId)}/export?${params.toString()}`;
  const response = await fetch(`${getBaseUrl()}${path}`, {
    method: 'GET',
    headers: buildBackendAuthHeaders('GET'),
    cache: 'no-store',
    signal: options?.signal,
  });
  if (!response.ok) {
    const rawText = await response.text();
    let body: unknown = rawText;
    try {
      body = rawText ? JSON.parse(rawText) : rawText;
    } catch {
      // Keep plain-text backend errors readable.
    }
    notifyHttpAuthFailure(response.status, body);
    throw new BackendHttpError({ method: 'GET', path, status: response.status, body });
  }
  const exportData = await response.text();
  const filename = observationExportFilename(
    response.headers.get('Content-Disposition'),
    rootTurnId
  );

  if (target.kind === 'native') {
    const saved = await ipcBridge.fs.writeFile.invoke({ path: target.path, data: exportData });
    if (!saved) throw new Error('The observation export could not be written.');
    return selectedFilename(target.path, filename);
  }

  const writable = await target.handle.createWritable();
  try {
    await writable.write(exportData);
    await writable.close();
  } catch (error) {
    try {
      await writable.abort?.();
    } catch {
      // Preserve the original write error.
    }
    throw error;
  }
  return target.handle.name?.trim() || filename;
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
