/** withRetry: exponential backoff, jitter bounds and retryable gating. */
import { describe, expect, it, vi } from "vitest";
import { AppServerError, TransportError } from "@flowy-agent-store/protocol";
import { withRetry } from "./retry";

function appServerError(retryable: boolean): AppServerError {
  return new AppServerError({
    code: "overloaded",
    message: "try later",
    request_id: "req-1",
    retryable,
    details: {},
  });
}

describe("withRetry", () => {
  it("retries retryable failures and returns the eventual success", async () => {
    const delays: number[] = [];
    const operation = vi
      .fn<() => Promise<string>>()
      .mockRejectedValueOnce(new TransportError("receive", "socket closed", { retryable: true }))
      .mockRejectedValueOnce(appServerError(true))
      .mockResolvedValue("ok");

    const result = await withRetry(operation, {
      sleep: async (ms) => {
        delays.push(ms);
      },
      jitter: 0,
      baseDelayMs: 100,
    });

    expect(result).toBe("ok");
    expect(operation).toHaveBeenCalledTimes(3);
    expect(delays).toEqual([100, 200]);
  });

  it("does not retry non-retryable protocol errors", async () => {
    const operation = vi.fn<() => Promise<string>>().mockRejectedValue(appServerError(false));
    const sleep = vi.fn(async () => undefined);

    await expect(withRetry(operation, { sleep })).rejects.toThrow("try later");
    expect(operation).toHaveBeenCalledTimes(1);
    expect(sleep).not.toHaveBeenCalled();
  });

  it("stops at maxAttempts and surfaces the last error", async () => {
    const operation = vi.fn<() => Promise<string>>().mockRejectedValue(appServerError(true));
    const delays: number[] = [];

    await expect(
      withRetry(operation, {
        maxAttempts: 2,
        baseDelayMs: 50,
        jitter: 0,
        sleep: async (ms) => {
          delays.push(ms);
        },
      }),
    ).rejects.toThrow("try later");
    expect(operation).toHaveBeenCalledTimes(2);
    expect(delays).toEqual([50]);
  });

  it("caps the delay at maxDelayMs and reports retries via onRetry", async () => {
    const operation = vi
      .fn<() => Promise<string>>()
      .mockRejectedValueOnce(appServerError(true))
      .mockRejectedValueOnce(appServerError(true))
      .mockRejectedValueOnce(appServerError(true))
      .mockResolvedValue("ok");
    const delays: number[] = [];
    const retries: number[] = [];

    await withRetry(operation, {
      maxAttempts: 4,
      baseDelayMs: 100,
      maxDelayMs: 150,
      jitter: 0,
      sleep: async (ms) => {
        delays.push(ms);
      },
      onRetry: ({ attempt }) => retries.push(attempt),
    });

    expect(delays).toEqual([100, 150, 150]);
    expect(retries).toEqual([1, 2, 3]);
  });

  it("keeps jittered delays within the documented band", async () => {
    const delays: number[] = [];
    const operation = vi
      .fn<() => Promise<string>>()
      .mockRejectedValueOnce(appServerError(true))
      .mockResolvedValue("ok");

    await withRetry(operation, {
      baseDelayMs: 1000,
      jitter: 0.25,
      sleep: async (ms) => {
        delays.push(ms);
      },
    });

    expect(delays).toHaveLength(1);
    expect(delays[0]).toBeGreaterThanOrEqual(750);
    expect(delays[0]).toBeLessThanOrEqual(1000);
  });
});
