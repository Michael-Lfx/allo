import { describe, expect, it } from "vitest";

import { CONTEXT_NEAR_LIMIT_PERCENT, contextAdvice } from "./context-advice";

describe("contextAdvice", () => {
  it("computes the percentage when the server did not measure one", () => {
    expect(contextAdvice({ used_tokens: 64_000, window_tokens: 128_000 })).toEqual({ level: "ok", percent: 50 });
  });

  it("prefers the measured percentage", () => {
    expect(contextAdvice({ used_tokens: 1, window_tokens: 128_000, percent: 91 })).toEqual({ level: "near", percent: 91 });
  });

  it("flips to `near` exactly at the threshold", () => {
    const at = contextAdvice({ used_tokens: 80, window_tokens: 100 });
    expect(at).toEqual({ level: "near", percent: CONTEXT_NEAR_LIMIT_PERCENT });
    const below = contextAdvice({ used_tokens: 79, window_tokens: 100 });
    expect(below?.level).toBe("ok");
  });

  it("clamps out-of-range percentages instead of showing 140%", () => {
    expect(contextAdvice({ used_tokens: 200, window_tokens: 100 })?.percent).toBe(100);
    expect(contextAdvice({ used_tokens: 0, window_tokens: 100, percent: -5 })?.percent).toBe(0);
  });

  it("stays silent when nothing was measured", () => {
    expect(contextAdvice(null)).toBeNull();
    expect(contextAdvice(undefined)).toBeNull();
    expect(contextAdvice({ used_tokens: 10, window_tokens: 0 })).toBeNull();
    expect(contextAdvice({ used_tokens: 10, window_tokens: Number.NaN })).toBeNull();
  });
});
