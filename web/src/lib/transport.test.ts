import { describe, expect, it } from "vitest";
import { isLoopbackUrl } from "./transport";

describe("isLoopbackUrl", () => {
  it("accepts loopback hosts", () => {
    expect(isLoopbackUrl("ws://127.0.0.1:8787/api/app-server/ws")).toBe(true);
    expect(isLoopbackUrl("http://127.0.0.2:80/")).toBe(true);
    expect(isLoopbackUrl("ws://localhost:8787/x")).toBe(true);
    expect(isLoopbackUrl("ws://[::1]:8787/x")).toBe(true);
  });

  it("rejects non-loopback and garbage", () => {
    expect(isLoopbackUrl("ws://192.168.1.10:8787/x")).toBe(false);
    expect(isLoopbackUrl("https://example.com/api")).toBe(false);
    expect(isLoopbackUrl("ws://127.0.0.1.evil.com/x")).toBe(false);
    expect(isLoopbackUrl("not a url")).toBe(false);
  });
});
