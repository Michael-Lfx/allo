/**
 * Retry helper (REQ-PAR-05e, ≈ Codex `retry_on_overload`): exponential backoff
 * with jitter, driven by the protocol's stable `retryable` hint.
 *
 * Only errors the server/transport marks retryable are retried; protocol
 * errors that carry `retryable: false` (validation, not_found, ...) surface
 * immediately. `command_id` / `idempotency_key` callers stay safe because the
 * App Server deduplicates replayed mutations.
 */

import { isRetryableError } from "@flowy-agent-store/protocol";

export interface RetryInfo {
  /** 1-based attempt that just failed and triggered the next try. */
  attempt: number;
  /** Delay applied before the next attempt. */
  delayMs: number;
  error: unknown;
}

export interface RetryOptions {
  /** Total attempts including the first (default 3). */
  maxAttempts?: number;
  /** First backoff delay in ms (default 500). */
  baseDelayMs?: number;
  /** Upper bound for one delay in ms (default 8000). */
  maxDelayMs?: number;
  /** Jitter fraction in [0, 1] (default 0.25 → delay ∈ [0.75×, 1.0×]). */
  jitter?: number;
  /** Observer hook for logging/metrics. */
  onRetry?: (info: RetryInfo) => void;
  /** Custom predicate; defaults to the protocol `retryable` hint. */
  shouldRetry?: (error: unknown, attempt: number) => boolean;
  /** Deterministic delay override (tests). */
  sleep?: (ms: number) => Promise<void>;
}

/** Run `operation`, retrying retryable failures with exponential backoff. */
export async function withRetry<T>(
  operation: () => Promise<T>,
  options: RetryOptions = {},
): Promise<T> {
  const maxAttempts = Math.max(1, options.maxAttempts ?? 3);
  const baseDelayMs = Math.max(0, options.baseDelayMs ?? 500);
  const maxDelayMs = Math.max(baseDelayMs, options.maxDelayMs ?? 8000);
  const jitter = Math.min(1, Math.max(0, options.jitter ?? 0.25));
  const sleep = options.sleep ?? ((ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms)));
  const shouldRetry = options.shouldRetry ?? ((error: unknown) => isRetryableError(error));

  let lastError: unknown;
  for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
    try {
      return await operation();
    } catch (error) {
      lastError = error;
      if (attempt >= maxAttempts || !shouldRetry(error, attempt)) {
        throw error;
      }
      const ceiling = Math.min(maxDelayMs, baseDelayMs * 2 ** (attempt - 1));
      const delayMs = Math.round(ceiling * (1 - jitter) + Math.random() * ceiling * jitter);
      options.onRetry?.({ attempt, delayMs, error });
      await sleep(delayMs);
    }
  }
  throw lastError;
}
