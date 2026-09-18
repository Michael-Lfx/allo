import type { AgentStreamErrorInfo } from '@/common/chat/chatLib';
import { isBackendHttpError, redactSensitiveText } from '@/common/adapter/httpBridge';

const MAX_DETAIL_CHARS = 1000;
const MAX_SUMMARY_CHARS = 240;
const MAX_SERIALIZED_DETAIL_CHARS = 8000;
const MAX_SERIALIZED_STRING_CHARS = 512;
const MAX_SERIALIZED_NODES = 256;
const MAX_SERIALIZED_DEPTH = 5;

export type ErrorDiagnosticInput = {
  message?: string;
  summary?: string;
  modelId?: string;
  providerId?: string;
  code?: string;
  incidentId?: string;
  ownership?: string;
  retryable?: boolean;
  resolutionKind?: string;
  resolutionTarget?: string;
  detail?: string;
  status?: number;
};

export type ErrorDiagnosticLabels = {
  modelId: string;
  providerId: string;
  errorCode: string;
  incidentId: string;
  ownership: string;
  httpStatus: string;
  retryable: string;
  resolution: string;
  resolutionTarget: string;
  detail: string;
  message: string;
  summary: string;
};

export type SafeErrorDiagnostic = {
  summary: string;
  message?: string;
  modelId?: string;
  providerId?: string;
  detail?: string;
  code?: string;
  incidentId?: string;
  ownership?: string;
  retryable?: boolean;
  resolutionKind?: string;
  resolutionTarget?: string;
  status?: number;
};

const truncate = (value: string, maxChars: number): string => {
  if (value.length <= maxChars) return value;
  let prefix = value.slice(0, maxChars);
  if (/[\uD800-\uDBFF]$/u.test(prefix)) prefix = prefix.slice(0, -1);
  return `${prefix}...`;
};

const SENSITIVE_DIAGNOSTIC_KEY_PATTERN =
  /api[_-]?key|authorization|auth[_-]?token|access[_-]?token|refresh[_-]?token|viewer[_-]?token|csrf[_-]?token|session[_-]?token|token|secret|password|credential|cookie|user[_ -]?input|prompt|workspace|working[_ -]?directory|cwd|path|raw[_ -]?response|response[_ -]?body|stack|trace/i;

const redactSensitiveAssignment = (value: string): string => {
  const authorizationRedacted = value.replace(
    /((?:["']?authorization["']?\s*[:=]\s*))(?:(["'])([^\r\n]*?)\2|([^\r\n,;}]+))/gi,
    (_match, prefix: string, quote: string | undefined, quotedValue: string | undefined, rawValue: string | undefined) => {
      const original = quotedValue ?? rawValue ?? '';
      const scheme = original.trim().split(/\s+/u)[0];
      const replacement = /^bearer$/iu.test(scheme) ? 'Bearer ' : '';
      return `${prefix}${quote ?? ''}${replacement}[REDACTED]${quote ?? ''}`;
    }
  );

  return authorizationRedacted
    .replace(
      /((?:["']?(?:api[_-]?key|auth[_-]?token|access[_-]?token|refresh[_-]?token|viewer[_-]?token|csrf[_-]?token|session[_-]?token|token|secret|password|credential|cookie|user[_ -]?input|prompt|workspace[_ -]?path|working[_ -]?directory|cwd|path|stack|trace|raw[_ -]?response|response[_ -]?body)["']?\s*[:=]\s*))(["'])[^\r\n]*?\2/gi,
      '$1$2[REDACTED]$2'
    )
    .replace(
      /((?:["']?(?:api[_-]?key|auth[_-]?token|access[_-]?token|refresh[_-]?token|viewer[_-]?token|csrf[_-]?token|session[_-]?token|token|secret|password|credential|cookie|user[_ -]?input|prompt|workspace[_ -]?path|working[_ -]?directory|cwd|path|stack|trace|raw[_ -]?response|response[_ -]?body)["']?\s*[:=]\s*))(?!["'])[^\r\n,;}]+/gi,
      '$1[REDACTED]'
    );
};

const safeText = (value: string | undefined, maxChars: number): string | undefined => {
  if (typeof value !== 'string' || !value.trim()) return undefined;
  const redacted = redactSensitiveAssignment(redactSensitiveText(value))
    .replace(
      /((?:workspace[_ -]?path|working[_ -]?directory|cwd)\s*[:=]\s*)[^\r\n,;}]+/gi,
      '$1[REDACTED_PATH]'
    )
    .replace(/(?:[A-Za-z]:\\)[^\r\n"'<>]+/g, '[REDACTED_PATH]')
    .replace(/\\\\[^\\/\s"'<>]+[\\/][^\r\n"'<>]+/g, '[REDACTED_PATH]')
    .replace(/file:\/\/\/[^\r\n"'<>]+/gi, '[REDACTED_PATH]')
    .replace(/\/(?:Users|home|root|private|var|tmp|opt|workspace)\/[^\r\n"'<>]+/g, '[REDACTED_PATH]')
    .trim();
  return redacted ? truncate(redacted, maxChars) : undefined;
};

type DiagnosticSerializationState = {
  nodes: number;
  seen: WeakSet<object>;
};

const serializeDiagnosticValue = (
  value: unknown,
  depth = 0,
  state: DiagnosticSerializationState = { nodes: 0, seen: new WeakSet<object>() }
): string => {
  if (state.nodes >= MAX_SERIALIZED_NODES) return '"[TRUNCATED]"';
  state.nodes += 1;

  if (value === null) return 'null';
  if (typeof value === 'string') return JSON.stringify(truncate(value, MAX_SERIALIZED_STRING_CHARS));
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  if (typeof value !== 'object') return '"[UNSUPPORTED]"';
  if (depth >= MAX_SERIALIZED_DEPTH) return '"[MAX_DEPTH]"';
  if (state.seen.has(value)) return '"[CIRCULAR]"';

  state.seen.add(value);
  let result: string;
  if (Array.isArray(value)) {
    const entries = value
      .slice(0, 32)
      .map((entry) => serializeDiagnosticValue(entry, depth + 1, state));
    result = `[${entries.join(',')}${value.length > 32 ? ',"[TRUNCATED]"' : ''}]`;
  } else {
    const entries: string[] = [];
    let inspectedKeys = 0;
    let truncatedObject = false;
    for (const key in value) {
      if (!Object.prototype.hasOwnProperty.call(value, key)) continue;
      inspectedKeys += 1;
      if (inspectedKeys > 64 || entries.length >= 32) {
        truncatedObject = true;
        break;
      }
      if (SENSITIVE_DIAGNOSTIC_KEY_PATTERN.test(key)) {
        truncatedObject = true;
        continue;
      }
      let entry: unknown;
      try {
        entry = (value as Record<string, unknown>)[key];
      } catch {
        entry = '[UNAVAILABLE]';
      }
      entries.push(
        `${JSON.stringify(key)}:${serializeDiagnosticValue(entry, depth + 1, state)}`
      );
    }
    result = `{${entries.join(',')}${truncatedObject ? ',"[TRUNCATED]":"[REDACTED]"' : ''}}`;
  }
  state.seen.delete(value);
  return truncate(result, MAX_SERIALIZED_DETAIL_CHARS);
};

const safeDetailsText = (value: unknown): string | undefined => {
  if (typeof value === 'string') return safeText(value, MAX_DETAIL_CHARS);
  if (value == null) return undefined;
  return safeText(serializeDiagnosticValue(value), MAX_DETAIL_CHARS);
};

const firstLine = (value: string | undefined): string | undefined => {
  if (!value) return undefined;
  const line = value.split(/\r?\n/u).find((entry) => entry.trim());
  return line ? truncate(line.trim(), MAX_SUMMARY_CHARS) : undefined;
};

const appendLine = (lines: string[], label: string, value: string | number | boolean | undefined): void => {
  if (value === undefined || value === '') return;
  lines.push(`${label}: ${String(value)}`);
};

export const getErrorDiagnosticLabels = (t: (key: string) => string): ErrorDiagnosticLabels => ({
  modelId: t('conversation.agentError.modelId'),
  providerId: t('conversation.agentError.providerId'),
  errorCode: t('conversation.agentError.errorCode'),
  incidentId: t('conversation.agentError.incidentId'),
  ownership: t('conversation.agentError.diagnosticOwnership'),
  httpStatus: t('conversation.agentError.httpStatus'),
  retryable: t('conversation.agentError.diagnosticRetryable'),
  resolution: t('conversation.agentError.diagnosticResolution'),
  resolutionTarget: t('conversation.agentError.diagnosticResolutionTarget'),
  detail: t('conversation.agentError.diagnosticDetail'),
  message: t('conversation.agentError.diagnosticMessage'),
  summary: t('conversation.agentError.diagnosticSummary'),
});

export const formatErrorDiagnosticText = (
  diagnostic: SafeErrorDiagnostic,
  labels: ErrorDiagnosticLabels
): string => {
  const lines: string[] = [];

  appendLine(lines, labels.modelId, diagnostic.modelId);
  appendLine(lines, labels.providerId, diagnostic.providerId);
  appendLine(lines, labels.errorCode, diagnostic.code);
  appendLine(lines, labels.incidentId, diagnostic.incidentId);
  appendLine(lines, labels.ownership, diagnostic.ownership);
  appendLine(lines, labels.httpStatus, diagnostic.status);
  appendLine(lines, labels.retryable, diagnostic.retryable);
  appendLine(lines, labels.resolution, diagnostic.resolutionKind);
  appendLine(lines, labels.resolutionTarget, diagnostic.resolutionTarget);
  if (diagnostic.detail) lines.push(`${labels.detail}:\n${diagnostic.detail}`);
  else if (diagnostic.message) lines.push(`${labels.message}:\n${diagnostic.message}`);
  else if (diagnostic.summary) appendLine(lines, labels.summary, diagnostic.summary);

  return lines.join('\n');
};

export const buildErrorDiagnostic = (input: ErrorDiagnosticInput): SafeErrorDiagnostic => {
  const detail = safeText(input.detail, MAX_DETAIL_CHARS);
  const message = safeText(input.message, MAX_SUMMARY_CHARS);
  const modelId = safeText(input.modelId, MAX_SERIALIZED_STRING_CHARS);
  const providerId = safeText(input.providerId, MAX_SERIALIZED_STRING_CHARS);
  const summary = firstLine(safeText(input.summary, MAX_SUMMARY_CHARS)) ?? firstLine(detail) ?? message ?? '';
  const code = safeText(input.code, MAX_SUMMARY_CHARS);
  const incidentId = safeText(input.incidentId, MAX_SUMMARY_CHARS);
  const ownership = safeText(input.ownership, MAX_SUMMARY_CHARS);
  const resolutionKind = safeText(input.resolutionKind, MAX_SUMMARY_CHARS);
  const resolutionTarget = safeText(input.resolutionTarget, MAX_SUMMARY_CHARS);

  return {
    summary,
    ...(message ? { message } : {}),
    ...(modelId ? { modelId } : {}),
    ...(providerId ? { providerId } : {}),
    ...(detail ? { detail } : {}),
    ...(code ? { code } : {}),
    ...(incidentId ? { incidentId } : {}),
    ...(ownership ? { ownership } : {}),
    ...(input.retryable !== undefined ? { retryable: input.retryable } : {}),
    ...(resolutionKind ? { resolutionKind } : {}),
    ...(resolutionTarget ? { resolutionTarget } : {}),
    ...(input.status !== undefined ? { status: input.status } : {}),
  };
};

export const buildAgentErrorDiagnostic = (error: AgentStreamErrorInfo): SafeErrorDiagnostic =>
  buildErrorDiagnostic({
    message: error.message,
    modelId: error.model_id,
    providerId: error.provider_id,
    code: error.code,
    incidentId: error.incident_id,
    ownership: error.ownership,
    retryable: error.retryable,
    resolutionKind: error.resolution?.kind,
    resolutionTarget: error.resolution?.target,
    detail: error.detail,
  });

export const buildUnknownErrorDiagnostic = (error: unknown, fallback: string): SafeErrorDiagnostic => {
  if (isBackendHttpError(error)) {
    const backendMessage = typeof error.backendMessage === 'string' && error.backendMessage.trim()
      ? error.backendMessage
      : undefined;
    return buildErrorDiagnostic({
      message: backendMessage || fallback,
      summary: backendMessage,
      code: error.code || undefined,
      detail: safeDetailsText(error.details),
      status: error.status,
    });
  }
  const message = error instanceof Error ? error.message : typeof error === 'string' ? error : undefined;
  return buildErrorDiagnostic({ message: message || fallback });
};
