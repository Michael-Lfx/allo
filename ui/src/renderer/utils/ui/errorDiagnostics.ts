import type { AgentStreamErrorInfo } from '@/common/chat/chatLib';
import { isBackendHttpError, redactSensitiveText } from '@/common/adapter/httpBridge';

const MAX_DETAIL_CHARS = 1000;
const MAX_SUMMARY_CHARS = 240;

export type ErrorDiagnosticInput = {
  message?: string;
  summary?: string;
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
  const chars = [...value];
  return chars.length > maxChars ? `${chars.slice(0, maxChars).join('')}...` : value;
};

const safeText = (value: string | undefined, maxChars: number): string | undefined => {
  if (typeof value !== 'string' || !value.trim()) return undefined;
  const redacted = redactSensitiveText(value)
    .replace(
      /((?:["']?authorization["']?\s*[:=]\s*["']?))([^"\r\n',;}]+)/gi,
      (match: string, prefix: string, value: string) => {
        const scheme = value.trim().split(/\s+/u)[0];
        return `${prefix}${scheme || '[REDACTED]'} [REDACTED]`;
      }
    )
    .replace(
      /((?:["']?(?:api[_-]?key|authorization|access[_-]?token|refresh[_-]?token|token|secret|password|credential|cookie|user[_-]?input|prompt|workspace[_-]?path|path)["']?\s*[:=]\s*["']?))([^"',}\s]+)(["']?)/gi,
      (match: string, prefix: string, secret: string, quote: string) =>
        secret.toLowerCase() === 'bearer' ? match : `${prefix}[REDACTED]${quote}`
    )
    .replace(/((?:workspace[_ -]?path|working[_ -]?directory|cwd)\s*[:=]\s*)[^\r\n,;}]+/gi, '$1[REDACTED_PATH]')
    .replace(/(?:[A-Za-z]:\\|\/(?:Users|home|root|private|var|tmp|opt|workspace)\/)[^\s"'<>]+/g, '[REDACTED_PATH]')
    .trim();
  return redacted ? truncate(redacted, maxChars) : undefined;
};

const safeDetailsText = (value: unknown): string | undefined => {
  if (typeof value === 'string') return safeText(value, MAX_DETAIL_CHARS);
  if (value == null) return undefined;
  try {
    return safeText(JSON.stringify(value, null, 2), MAX_DETAIL_CHARS);
  } catch {
    return undefined;
  }
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
  const summary = firstLine(safeText(input.summary, MAX_SUMMARY_CHARS)) ?? firstLine(detail) ?? message ?? '';
  const code = safeText(input.code, MAX_SUMMARY_CHARS);
  const incidentId = safeText(input.incidentId, MAX_SUMMARY_CHARS);
  const ownership = safeText(input.ownership, MAX_SUMMARY_CHARS);
  const resolutionKind = safeText(input.resolutionKind, MAX_SUMMARY_CHARS);
  const resolutionTarget = safeText(input.resolutionTarget, MAX_SUMMARY_CHARS);

  return {
    summary,
    ...(message ? { message } : {}),
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
