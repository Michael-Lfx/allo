import { describe, expect, it } from "vitest";

import type { RunEvent } from "@flowy-agent-store/protocol";
import {
  RUN_TERMINAL_TONES,
  RUN_TERMINAL_TOAST_KEYS,
  TERMINAL_RUN_STATUSES,
  isBackgroundRun,
  isTerminalRunStatus,
  latestRunStatus,
  runTerminalNoticeKey,
  terminalRunStatus,
} from "./run-notify";

function event(sequence: number, event_type: string, status?: string, runId = "run-1"): RunEvent {
  return {
    run_id: runId,
    sequence,
    event_type,
    payload: status === undefined ? {} : { status },
  };
}

describe("latestRunStatus", () => {
  it("reads the status the stream reported last by sequence", () => {
    const events = [event(1, "run.started", "planning"), event(2, "run.status_changed", "running")];
    expect(latestRunStatus(events)).toBe("running");
  });

  it("orders by sequence, not by arrival order", () => {
    const events = [event(7, "run.status_changed", "completed"), event(3, "run.status_changed", "running")];
    expect(latestRunStatus(events)).toBe("completed");
  });

  it("ignores events without a run-level status", () => {
    const events = [
      event(1, "run.started", "planning"),
      event(2, "task.updated"),
      event(3, "approval.requested"),
      event(4, "attempt.updated"),
    ];
    expect(latestRunStatus(events)).toBe("planning");
  });

  it("ignores a non-string or empty status payload", () => {
    const events = [event(1, "run.started", "planning"), event(2, "run.status_changed"), event(3, "run.status_changed", "")];
    expect(latestRunStatus(events)).toBe("planning");
    expect(latestRunStatus([])).toBeNull();
  });
});

describe("terminalRunStatus", () => {
  it("stays null while the run may still move", () => {
    expect(terminalRunStatus([event(1, "run.started", "planning")])).toBeNull();
    expect(terminalRunStatus([event(1, "run.status_changed", "running")])).toBeNull();
    expect(terminalRunStatus([event(1, "run.status_changed", "waiting_input")])).toBeNull();
    expect(terminalRunStatus([event(1, "run.status_changed", "awaiting_approval")])).toBeNull();
  });

  it("reports every terminal status the engine emits", () => {
    for (const status of TERMINAL_RUN_STATUSES) {
      expect(terminalRunStatus([event(2, "run.status_changed", status)])).toBe(status);
      expect(isTerminalRunStatus(status)).toBe(true);
    }
  });

  it("does not treat an unknown status string as terminal", () => {
    expect(terminalRunStatus([event(2, "run.status_changed", "quantum")])).toBeNull();
    expect(isTerminalRunStatus("quantum")).toBe(false);
  });

  it("follows the latest status even when the run moved back to non-terminal", () => {
    const events = [event(2, "run.status_changed", "completed"), event(5, "run.status_changed", "running")];
    expect(terminalRunStatus(events)).toBeNull();
  });
});

describe("notice identity", () => {
  it("keys one reminder per (run, terminal status)", () => {
    expect(runTerminalNoticeKey("run-1", "completed")).toBe("run:run-1:terminal:completed");
    expect(runTerminalNoticeKey("run-1", "failed")).not.toBe(runTerminalNoticeKey("run-1", "completed"));
    expect(runTerminalNoticeKey("run-2", "failed")).not.toBe(runTerminalNoticeKey("run-1", "failed"));
  });

  it("covers every terminal status with a toast key and a tone", () => {
    for (const status of TERMINAL_RUN_STATUSES) {
      expect(RUN_TERMINAL_TOAST_KEYS[status]).toMatch(/^toast\.run/);
      expect(["success", "error"]).toContain(RUN_TERMINAL_TONES[status]);
    }
  });
});

describe("isBackgroundRun", () => {
  it("is true exactly when the in-tab toast would not be seen", () => {
    expect(isBackgroundRun({ hidden: false, focused: true })).toBe(false);
    expect(isBackgroundRun({ hidden: true, focused: true })).toBe(true);
    expect(isBackgroundRun({ hidden: false, focused: false })).toBe(true);
    expect(isBackgroundRun({ hidden: true, focused: false })).toBe(true);
  });
});
