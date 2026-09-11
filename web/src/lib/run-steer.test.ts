import { describe, expect, it } from "vitest";

import { STEER_ACCEPTED_KEY, STEER_BLOCKED_KEYS, isStaleRunWrite, steerAvailability } from "./run-steer";

describe("steerAvailability", () => {
  const running = { hasRun: true, busy: false, status: "running", terminal: false };

  it("is available for a live run", () => {
    expect(steerAvailability(running)).toBe("available");
    expect(steerAvailability({ ...running, status: "waiting_input" })).toBe("available");
    // Events have not caught up yet: still steerable, the server decides.
    expect(steerAvailability({ ...running, status: null })).toBe("available");
  });

  it("refuses a terminal run instead of sending a doomed request", () => {
    expect(steerAvailability({ ...running, status: "completed", terminal: true })).toBe("terminal");
    expect(steerAvailability({ ...running, status: "failed", terminal: true })).toBe("terminal");
  });

  it("reports the missing run and the in-flight submission", () => {
    expect(steerAvailability({ ...running, hasRun: false })).toBe("no-run");
    expect(steerAvailability({ ...running, busy: true })).toBe("busy");
  });

  it("orders the refusals: no run wins over terminal, terminal over busy", () => {
    expect(steerAvailability({ hasRun: false, busy: true, status: "completed", terminal: true })).toBe("no-run");
    expect(steerAvailability({ hasRun: true, busy: true, status: "failed", terminal: true })).toBe("terminal");
  });

  it("ships a localized reason for every refusal", () => {
    expect(Object.keys(STEER_BLOCKED_KEYS).sort()).toEqual(["busy", "no-run", "terminal"]);
    expect(STEER_BLOCKED_KEYS.terminal).toBe("run.steerTerminal");
    expect(STEER_ACCEPTED_KEY).toBe("run.steerAccepted");
  });
});

describe("isStaleRunWrite", () => {
  it("recognizes the CAS refusal", () => {
    expect(isStaleRunWrite({ code: "conflict", message: "step moved" })).toBe(true);
    expect(isStaleRunWrite(new Error("x"))).toBe(false);
  });

  it("never misreads other failures as stale", () => {
    expect(isStaleRunWrite(null)).toBe(false);
    expect(isStaleRunWrite(undefined)).toBe(false);
    expect(isStaleRunWrite("conflict")).toBe(false);
    expect(isStaleRunWrite({ code: "not_found" })).toBe(false);
    expect(isStaleRunWrite({ code: "runtime_unavailable" })).toBe(false);
    expect(isStaleRunWrite({ error: { code: "conflict" } })).toBe(false);
  });
});
