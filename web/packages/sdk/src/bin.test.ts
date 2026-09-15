import { afterEach, describe, expect, it, vi } from "vitest";
import { existsSync } from "node:fs";
import { resolveAppServerBin } from "./bin";

/**
 * `existsSync` is the resolver's only window on the filesystem, so it is mocked
 * with a *controllable* stand-in whose default implementation is the real one.
 *
 * Why this is not optional: the "nothing resolves" branch is unreachable in a
 * normally installed workspace. `@flowy-agent-store/runtime-<platform>` is both
 * an optionalDependency of this package and a workspace member, so `bun install`
 * links it and `findInRuntimePackage()` then always finds the vendored binary.
 * This file used to pass only because that link happened not to exist on the
 * machine running it — it went red the moment the lock was refreshed.
 */
vi.mock("node:fs", async (importOriginal) => {
  const actual = await importOriginal<typeof import("node:fs")>();
  return { ...actual, existsSync: vi.fn(actual.existsSync) };
});

const mockedExists = vi.mocked(existsSync);

const SAVED = process.env["AGENT_STORE_BIN"];

afterEach(async () => {
  const actual = await vi.importActual<typeof import("node:fs")>("node:fs");
  mockedExists.mockImplementation(actual.existsSync);
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
    // Everything — the explicit path, the runtime package, PATH — misses.
    mockedExists.mockReturnValue(false);
    expect(() => resolveAppServerBin("/definitely/not/here/agent-store")).toThrow(/cannot find/);
  });
});
