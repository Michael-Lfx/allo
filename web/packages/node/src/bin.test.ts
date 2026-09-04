import { afterEach, describe, expect, it } from "vitest";
import { resolveAppServerBin } from "./bin";

const SAVED = process.env["AGENT_STORE_BIN"];

afterEach(() => {
  if (SAVED === undefined) delete process.env["AGENT_STORE_BIN"];
  else process.env["AGENT_STORE_BIN"] = SAVED;
});

describe("resolveAppServerBin", () => {
  it("prefers an explicit path that exists", () => {
    expect(resolveAppServerBin(process.execPath)).toBe(process.execPath);
  });

  it("falls back to AGENT_STORE_BIN", () => {
    process.env["AGENT_STORE_BIN"] = process.execPath;
    expect(resolveAppServerBin()).toBe(process.execPath);
  });

  it("throws when nothing resolves", () => {
    delete process.env["AGENT_STORE_BIN"];
    expect(() => resolveAppServerBin("/definitely/not/here/agent-store")).toThrow(/cannot find/);
  });
});
