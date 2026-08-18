import { describe, expect, test } from 'bun:test';

import type { AgentStreamErrorInfo } from '@/common/chat/chatLib';

import {
  buildAgentErrorDiagnostic,
  buildErrorDiagnostic,
  buildUnknownErrorDiagnostic,
  formatErrorDiagnosticText,
  type ErrorDiagnosticLabels,
} from './errorDiagnostics';

const labels: ErrorDiagnosticLabels = {
  errorCode: 'Error code',
  incidentId: 'Incident ID',
  ownership: 'Ownership',
  httpStatus: 'HTTP status',
  retryable: 'Retryable',
  resolution: 'Resolution',
  resolutionTarget: 'Resolution target',
  detail: 'Detail',
  message: 'Message',
  summary: 'Summary',
};

describe('error diagnostics', () => {
  test('builds a concise summary and complete safe diagnostic text', () => {
    const error: AgentStreamErrorInfo = {
      message: 'The provider rejected the request',
      incident_id: 'incident-1',
      code: 'USER_LLM_PROVIDER_INVALID_TOOL_SCHEMA',
      ownership: 'user_llm_provider',
      detail: 'Invalid schema for function Read\nThe provider returned status 400.',
      retryable: false,
      resolution: { kind: 'change_model', target: 'provider_settings' },
    };

    const diagnostic = buildAgentErrorDiagnostic(error);
    const text = formatErrorDiagnosticText(diagnostic, labels);

    expect(diagnostic.summary).toBe('Invalid schema for function Read');
    expect(text).toContain('Error code: USER_LLM_PROVIDER_INVALID_TOOL_SCHEMA');
    expect(text).toContain('Incident ID: incident-1');
    expect(text).toContain('Resolution target: provider_settings');
    expect(text).toContain('The provider returned status 400.');
  });

  test('redacts credentials, URL queries, and caps long details', () => {
    const diagnostic = buildErrorDiagnostic({
      message: 'request failed',
      detail: `Authorization: Bearer secret\ntoken=secret\npassword=two words that must not leak\nhttps://provider.test/path?api_key=secret&keep=yes\n${'x'.repeat(1200)}`,
    });
    const text = formatErrorDiagnosticText(diagnostic, labels);

    expect(text).not.toContain('Bearer secret');
    expect(text).not.toContain('token=secret');
    expect(text).not.toContain('two words that must not leak');
    expect(text).not.toContain('api_key=secret');
    expect(diagnostic.detail?.length).toBeLessThanOrEqual(1003);
    expect(diagnostic.detail).toContain('...');
  });

  test('falls back to the message when detail is unavailable', () => {
    const diagnostic = buildErrorDiagnostic({ message: 'A safe fallback message' });
    const text = formatErrorDiagnosticText(diagnostic, labels);

    expect(diagnostic.summary).toBe('A safe fallback message');
    expect(text).toContain('Message:\nA safe fallback message');
  });

  test('reads safe BackendHttpError fields without serializing the whole error', () => {
    const error = new Error('transport message') as Error & {
      name: string;
      status: number;
      code: string;
      backendMessage: string;
      details: unknown;
    };
    error.name = 'BackendHttpError';
    error.status = 422;
    error.code = 'INVALID_REQUEST';
    error.backendMessage = 'The request was rejected';
    error.details = { reason: 'bad input', token: 'Bearer secret' };

    const diagnostic = buildUnknownErrorDiagnostic(error, 'Fallback');
    const text = formatErrorDiagnosticText(diagnostic, labels);

    expect(diagnostic.summary).toBe('The request was rejected');
    expect(diagnostic.status).toBe(422);
    expect(text).not.toContain('Bearer secret');
  });

  test('uses the safe fallback instead of a transport message containing the response body', () => {
    const error = new Error(
      'Backend GET /api/internal failed (500): {"raw_response":"response-body-sentinel","user_input":"secret"}'
    ) as Error & {
      name: string;
      status: number;
      code: string;
      backendMessage: string;
      details: unknown;
    };
    error.name = 'BackendHttpError';
    error.status = 500;
    error.code = 'INTERNAL_ERROR';
    error.backendMessage = '';
    error.details = undefined;

    const diagnostic = buildUnknownErrorDiagnostic(error, '请求失败');
    const text = formatErrorDiagnosticText(diagnostic, labels);

    expect(diagnostic.summary).toBe('请求失败');
    expect(text).not.toContain('response-body-sentinel');
    expect(text).not.toContain('/api/internal');
  });

  test('redacts complete non-Bearer Authorization values', () => {
    const diagnostic = buildErrorDiagnostic({
      message: 'request failed',
      detail: 'Authorization: Basic super-secret-credential\n"authorization":"Digest another-secret"',
    });
    const text = formatErrorDiagnosticText(diagnostic, labels);

    expect(text).not.toContain('super-secret-credential');
    expect(text).not.toContain('another-secret');
  });

  test('does not expose workspace paths from structured backend details', () => {
    const error = new Error('request failed') as Error & {
      name: string;
      status: number;
      code: string;
      backendMessage: string;
      details: unknown;
    };
    error.name = 'BackendHttpError';
    error.status = 500;
    error.code = 'INTERNAL_ERROR';
    error.backendMessage = 'The request failed';
    error.details = { workspace_path: 'C:\\Users\\secret\\workspace', reason: 'safe diagnostic' };

    const diagnostic = buildUnknownErrorDiagnostic(error, 'Fallback');
    const text = formatErrorDiagnosticText(diagnostic, labels);

    expect(text).not.toContain('C:\\Users\\secret\\workspace');
    expect(text).toContain('safe diagnostic');
  });

  test('redacts space-containing secrets, user input, UNC paths, file URIs, and cycles', () => {
    const details: Record<string, unknown> = {
      password: 'two words that must not leak',
      user_input: 'private phrase from the composer',
      unc_path: '\\\\server\\share\\private\\file.txt',
      file_uri: 'file:///Users/secret/private/file.txt',
      stack: 'Error: private stack\n    at privateFunction (C:\\Users\\secret\\app.js:1:1)',
      safe_reason: 'provider rejected the schema',
    };
    details.circular = details;

    const error = Object.assign(new Error('request failed'), {
      name: 'BackendHttpError',
      status: 500,
      code: 'INTERNAL_ERROR',
      backendMessage: 'The request failed',
      details,
    });

    const diagnostic = buildUnknownErrorDiagnostic(error, 'Fallback');
    const text = formatErrorDiagnosticText(diagnostic, labels);

    expect(text).not.toContain('two words that must not leak');
    expect(text).not.toContain('private phrase from the composer');
    expect(text).not.toContain('server\\share\\private\\file.txt');
    expect(text).not.toContain('file:///Users/secret/private/file.txt');
    expect(text).not.toContain('privateFunction');
    expect(text).toContain('provider rejected the schema');
    expect(diagnostic.detail?.length).toBeLessThanOrEqual(1003);
  });
});
