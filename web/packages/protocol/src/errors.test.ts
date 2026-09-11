import { describe, expect, it } from "vitest";

import {
  AppServerError,
  ProtocolError,
  RequestTimeoutError,
  TransportError,
  formatError,
  isAppServerError,
  isRetryableError,
  isRetryableTransportError,
} from "./errors";
import type { WireError } from "./protocol";

function wireError(overrides: Partial<WireError> = {}): WireError {
  return { code: "not_found", message: "conversation not found", retryable: false, ...overrides } as WireError;
}

describe("protocol error model", () => {
  it("keeps the stable code, request id and retry hint from the wire", () => {
    const error = new AppServerError(
      wireError({ code: "busy", message: "runtime busy", retryable: true, request_id: "req-7", details: { retry_after_ms: 120 } }),
    );
    expect(error.name).toBe("AppServerError");
    expect(error.code).toBe("busy");
    expect(error.requestId).toBe("req-7");
    expect(error.retryable).toBe(true);
    expect(error.details).toEqual({ retry_after_ms: 120 });
  });

  it("defaults missing details to an empty object instead of undefined", () => {
    expect(new AppServerError(wireError()).details).toEqual({});
  });

  it("classifies transport failures by phase and defaults to non-retryable", () => {
    const fatal = new TransportError("send", "socket closed");
    const retryable = new TransportError("connect", "handshake failed", { retryable: true, cause: new Error("ECONNRESET") });
    expect(fatal.phase).toBe("send");
    expect(fatal.retryable).toBe(false);
    expect(retryable.retryable).toBe(true);
    expect(retryable.cause).toBeInstanceOf(Error);
  });

  it("classifies protocol violations by kind", () => {
    for (const kind of ["invalid_message", "version_mismatch", "unexpected_response"] as const) {
      expect(new ProtocolError(kind, kind).kind).toBe(kind);
    }
  });

  it("carries the timed-out method and budget", () => {
    const error = new RequestTimeoutError("conversation/send", 30_000);
    expect(error.method).toBe("conversation/send");
    expect(error.timeoutMs).toBe(30_000);
    expect(error.message).toContain("conversation/send");
  });

  it("does not mistake one error class for another", () => {
    expect(isAppServerError(new TransportError("receive", "x"))).toBe(false);
    expect(isAppServerError({ code: "not_found", message: "looks like a wire error" })).toBe(false);
    expect(isRetryableTransportError(new TransportError("receive", "x", { retryable: true }))).toBe(true);
    expect(isRetryableTransportError(new AppServerError(wireError({ retryable: true })))).toBe(false);
  });

  it("treats the retry hint as the single source of truth", () => {
    expect(isRetryableError(new AppServerError(wireError({ retryable: true })))).toBe(true);
    expect(isRetryableError(new AppServerError(wireError({ retryable: false })))).toBe(false);
    expect(isRetryableError(new TransportError("close", "x", { retryable: true }))).toBe(true);
    expect(isRetryableError(new Error("plain"))).toBe(false);
    expect(isRetryableError("not an error")).toBe(false);
  });

  it("renders one shape per class, with the code leading for wire errors", () => {
    expect(formatError(new AppServerError(wireError()))).toBe("not_found: conversation not found");
    expect(formatError(new Error("boom"))).toBe("boom");
    expect(formatError("raw string")).toBe("raw string");
  });
});
