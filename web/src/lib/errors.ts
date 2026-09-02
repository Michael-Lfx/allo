/**
 * App Server client error model.
 *
 * Callers must branch on the stable `code`, never on the human-readable
 * `message`. `retryable` mirrors the server-side retry hint; idempotency
 * conflicts and policy denials are never retried automatically.
 */

import type { WireError } from "./protocol";

export type ErrorCode = string;

export class AppServerError extends Error {
  readonly code: ErrorCode;
  readonly requestId?: string | number | null;
  readonly retryable: boolean;
  readonly details: Record<string, unknown>;

  constructor(wire: WireError) {
    super(wire.message);
    this.name = "AppServerError";
    this.code = wire.code;
    this.requestId = wire.request_id;
    this.retryable = wire.retryable;
    this.details = wire.details ?? {};
  }
}

export type TransportPhase = "connect" | "send" | "receive" | "close";

export class TransportError extends Error {
  readonly phase: TransportPhase;
  readonly retryable: boolean;
  readonly cause?: unknown;

  constructor(phase: TransportPhase, message: string, options?: { retryable?: boolean; cause?: unknown }) {
    super(message);
    this.name = "TransportError";
    this.phase = phase;
    this.retryable = options?.retryable ?? false;
    this.cause = options?.cause;
  }
}

export type ProtocolErrorKind =
  | "invalid_message"
  | "version_mismatch"
  | "unexpected_response";

export class ProtocolError extends Error {
  readonly kind: ProtocolErrorKind;

  constructor(kind: ProtocolErrorKind, message: string) {
    super(message);
    this.name = "ProtocolError";
    this.kind = kind;
  }
}

export class RequestTimeoutError extends Error {
  readonly method: string;
  readonly timeoutMs: number;

  constructor(method: string, timeoutMs: number) {
    super(`app-server request timed out after ${timeoutMs}ms: ${method}`);
    this.name = "RequestTimeoutError";
    this.method = method;
    this.timeoutMs = timeoutMs;
  }
}

export function isAppServerError(error: unknown): error is AppServerError {
  return error instanceof AppServerError;
}

export function isRetryableTransportError(error: unknown): boolean {
  return error instanceof TransportError && error.retryable;
}