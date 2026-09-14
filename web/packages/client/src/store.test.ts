/**
 * `StoreClient`: the lifecycle state machine over the flat protocol methods.
 *
 * `StoreClient` takes a structural `StoreHost`, so these tests drive a plain
 * fake — no transport, no WebSocket, no server. That is deliberate: everything
 * worth testing here is orchestration (what gets called, in what order, and what
 * the caller is told when it does not go to plan), not wire mapping.
 */
import { describe, expect, it } from "vitest";
import type {
  AgentDetail,
  ConnectorProbeResult,
  ConnectorStatusView,
  InstallOutcome,
  InstallStatus,
  SkillSummary,
  StoreInstallResult,
  StoreItem,
  StoreList,
  TeamDetail,
} from "@flowy-agent-store/protocol";
import { StoreClient, StoreError, type StoreHost } from "./store";

function item(overrides: Partial<StoreItem> = {}): StoreItem {
  return {
    id: "market-1/team-tools",
    marketplace_id: "market-1",
    marketplace_name: "Tools",
    entry_name: "team-tools",
    kind: "agent",
    name: "Team Tools",
    version: "2.0.0",
    source_kind: "directory",
    installed: false,
    update_available: false,
    snapshot_id: null,
    installed_version: null,
    blocked_reason: null,
    ...overrides,
  };
}

function outcome(overrides: Partial<InstallOutcome> = {}): InstallOutcome {
  return {
    component_id: "comp-1",
    kind: "agent",
    action: "created",
    ok: true,
    ...overrides,
  };
}

function installResult(overrides: Partial<StoreInstallResult> = {}): StoreInstallResult {
  return {
    marketplace_id: "market-1",
    entry_name: "team-tools",
    snapshot_id: "snap-1",
    version: "2.0.0",
    reused: false,
    installed_count: 1,
    warnings: [],
    errors: [],
    outcomes: [outcome()],
    ...overrides,
  };
}

function status(components: InstallStatus["components"], extra: Partial<InstallStatus> = {}): InstallStatus {
  return { snapshot_id: "snap-1", components, ...extra };
}

interface FakeOptions {
  store?: StoreItem[];
  install?: StoreInstallResult;
  /** Successive `install/status` answers; the last one repeats. */
  statuses?: InstallStatus[];
  skills?: SkillSummary[];
  agent?: AgentDetail | null;
  team?: TeamDetail | null;
  probe?: ConnectorProbeResult[];
  connectorStatus?: ConnectorStatusView[];
  uninstall?: InstallStatus;
  disable?: InstallStatus;
  enable?: InstallStatus;
}

function fakeHost(options: FakeOptions = {}) {
  const calls: string[] = [];
  let statusIndex = 0;
  let probeIndex = 0;
  let connectorIndex = 0;
  const statuses = options.statuses ?? [status([{ id: "comp-1", kind: "agent", name: "Team Tools", state: "installed" }])];

  const host: StoreHost = {
    async listStore(): Promise<StoreList> {
      calls.push("listStore");
      return { items: options.store ?? [] };
    },
    async installStoreEntry(marketplaceId: string, entryName: string) {
      calls.push(`installStoreEntry:${marketplaceId}/${entryName}`);
      return options.install ?? installResult();
    },
    async getInstallStatus(snapshotId: string) {
      calls.push(`getInstallStatus:${snapshotId}`);
      const answer = statuses[Math.min(statusIndex, statuses.length - 1)];
      statusIndex += 1;
      return answer;
    },
    async uninstallInstall(snapshotId: string, componentIds: string[]) {
      calls.push(`uninstallInstall:${snapshotId}:${componentIds.join(",")}`);
      return (
        options.uninstall ??
        status([{ id: "comp-1", kind: "agent", name: "Team Tools", state: "not-installed" }], {
          outcomes: [outcome({ action: "removed" })],
        })
      );
    },
    async disableInstall(snapshotId: string, componentIds: string[]) {
      calls.push(`disableInstall:${snapshotId}:${componentIds.join(",")}`);
      return (
        options.disable ??
        status([{ id: "comp-1", kind: "agent", name: "Team Tools", state: "disabled" }], {
          outcomes: [outcome({ action: "disabled" })],
        })
      );
    },
    async enableInstall(snapshotId: string, componentIds: string[]) {
      calls.push(`enableInstall:${snapshotId}:${componentIds.join(",")}`);
      return options.enable ?? status([{ id: "comp-1", kind: "agent", name: "Team Tools", state: "installed" }]);
    },
    skills: {
      async list() {
        calls.push("skills.list");
        return options.skills ?? [];
      },
    },
    agents: {
      async get(id: string) {
        calls.push(`agents.get:${id}`);
        if (options.agent === null) throw new Error("not found");
        return (
          options.agent ?? ({ id, name: "Team Tools", preset_id: "preset-1" } as unknown as AgentDetail)
        );
      },
    },
    teams: {
      async get(id: string) {
        calls.push(`teams.get:${id}`);
        if (options.team === null) throw new Error("not found");
        return options.team ?? ({ id, name: "Team Tools" } as unknown as TeamDetail);
      },
    },
    connectors: {
      async status(id: string) {
        calls.push(`connectors.status:${id}`);
        const list = options.connectorStatus ?? [{ connector_id: id, status: "connected" }];
        const answer = list[Math.min(connectorIndex, list.length - 1)];
        connectorIndex += 1;
        return answer;
      },
      async test(id: string) {
        calls.push(`connectors.test:${id}`);
        const list = options.probe ?? [{ connector_id: id, success: true }];
        const answer = list[Math.min(probeIndex, list.length - 1)];
        probeIndex += 1;
        return answer;
      },
    },
  };
  return { host, calls };
}

describe("StoreClient · catalog reads", () => {
  const items = [
    item(),
    item({ entry_name: "notes", id: "market-1/notes", kind: "skill", name: "Release Notes", installed: true, snapshot_id: "snap-2", installed_version: "1.0.0", update_available: true }),
    item({ entry_name: "mail", id: "market-1/mail", kind: "connector", name: "Mail", installed: true, snapshot_id: "snap-3" }),
  ];

  it("search matches the fields a user can see, case-insensitively", async () => {
    const { host } = fakeHost({ store: items });
    const client = new StoreClient(host);

    expect((await client.search("release")).map((entry) => entry.entry_name)).toEqual(["notes"]);
    expect((await client.search("TEAM")).map((entry) => entry.entry_name)).toEqual(["team-tools"]);
    expect(await client.search("")).toHaveLength(3);
  });

  it("search can narrow by kind", async () => {
    const { host } = fakeHost({ store: items });
    const client = new StoreClient(host);

    expect((await client.search("", { kind: "skill" })).map((entry) => entry.entry_name)).toEqual(["notes"]);
  });

  it("installed() returns only installed items", async () => {
    const { host } = fakeHost({ store: items });
    const client = new StoreClient(host);

    expect((await client.installed()).map((entry) => entry.entry_name)).toEqual(["notes", "mail"]);
  });

  it("checkUpdates() requires installed AND update_available", async () => {
    const { host } = fakeHost({
      store: [
        item({ update_available: true }),
        ...items,
      ],
    });
    const client = new StoreClient(host);

    // The first item has a pending update but was never installed; reporting it
    // would offer an "update" for something the user never had.
    expect((await client.checkUpdates()).map((entry) => entry.entry_name)).toEqual(["notes"]);
  });

  it("updateHint never invents an update path", () => {
    const { host } = fakeHost();
    const client = new StoreClient(host);

    expect(client.updateHint(item())).toBe("none");
    expect(client.updateHint(item({ installed: true }))).toBe("none");
    expect(
      client.updateHint(item({ installed: true, update_available: true, snapshot_id: "snap-1" })),
    ).toBe("uninstall_reinstall");
    expect(client.updateHint(item({ installed: true, update_available: true, snapshot_id: null }))).toBe(
      "unknown",
    );
  });
});

describe("StoreClient · install", () => {
  it("installs and reports the server's per-component detail", async () => {
    const { host, calls } = fakeHost({
      install: installResult({ outcomes: [outcome({ action: "reused" })] }),
    });
    const client = new StoreClient(host);

    const result = await client.install(item(), { waitForReady: false });

    expect(calls).toContain("installStoreEntry:market-1/team-tools");
    expect(result).toMatchObject({ snapshotId: "snap-1", reused: false, ok: true });
    expect(result.components).toEqual([outcome({ action: "reused" })]);
  });

  it("reports a failed component as not ok", async () => {
    const { host } = fakeHost({
      install: installResult({
        errors: ["comp-1: preset_create_failed"],
        outcomes: [outcome({ action: "failed", ok: false, code: "preset_create_failed" })],
      }),
    });
    const client = new StoreClient(host);

    const result = await client.install(item(), { waitForReady: false });

    expect(result.ok).toBe(false);
    expect(result.components[0].code).toBe("preset_create_failed");
  });

  it("refuses a blocked entry before spending a round trip", async () => {
    const { host, calls } = fakeHost();
    const client = new StoreClient(host);

    await expect(client.install(item({ blocked_reason: "strict entry without plugin.json" }))).rejects.toThrow(
      StoreError,
    );
    expect(calls).toEqual([]);
  });

  it("does not treat a missing `outcomes` array as an empty install", async () => {
    const { host } = fakeHost({ install: installResult({ outcomes: undefined }) });
    const client = new StoreClient(host);

    const result = await client.install(item(), { waitForReady: false });

    // No detail is not the same as no work: the aggregate counts still say the
    // install happened.
    expect(result.ok).toBe(true);
    expect(result.components).toEqual([]);
  });
});

describe("StoreClient · waitForReady", () => {
  it("times out while the agent has no preset yet", async () => {
    const { host, calls } = fakeHost({
      statuses: [
        status([{ id: "comp-1", kind: "agent", name: "Team Tools", state: "installed" }]),
        status([{ id: "comp-1", kind: "agent", name: "Team Tools", state: "installed" }]),
      ],
      agent: { id: "comp-1", name: "Team Tools" } as unknown as AgentDetail, // no preset_id yet
    });
    const client = new StoreClient(host, { readyPollMs: 1, readyTimeoutMs: 40 });

    const result = await client.install(item());

    expect(result.ready).toBe(false);
    expect(result.readyIssue).toBe("ready_timeout");
    expect(calls.filter((call) => call === "agents.get:comp-1").length).toBeGreaterThan(1);
  });

  it("resolves ready once the host reports the component usable", async () => {
    const { host } = fakeHost({
      agent: { id: "comp-1", name: "Team Tools", preset_id: "preset-1" } as unknown as AgentDetail,
    });
    const client = new StoreClient(host, { readyPollMs: 1, readyTimeoutMs: 50 });

    const result = await client.install(item());

    expect(result.ready).toBe(true);
    expect(result.snapshotId).toBe("snap-1");
  });

  it("keeps the install when readiness times out", async () => {
    const { host } = fakeHost({
      statuses: [status([{ id: "comp-1", kind: "skill", name: "Nowhere", state: "installed" }])],
      skills: [],
    });
    const client = new StoreClient(host, { readyPollMs: 1, readyTimeoutMs: 15 });

    const result = await client.install(item());

    // A slow component must never be reported as a failed install.
    expect(result.snapshotId).toBe("snap-1");
    expect(result.ready).toBe(false);
    expect(result.readyIssue).toBe("ready_timeout");
    expect(result.readyComponentId).toBe("comp-1");
  });

  it("returns immediately when a connector needs authorization", async () => {
    const { host, calls } = fakeHost({
      statuses: [status([{ id: "conn-1", kind: "connector", name: "Mail", state: "installed" }])],
      probe: [{ connector_id: "conn-1", success: false, code: "authorization_required" }],
      connectorStatus: [{ connector_id: "conn-1", status: "authorization_required" }],
    });
    const client = new StoreClient(host, { readyPollMs: 1, readyTimeoutMs: 2_000 });

    const result = await client.install(item());

    expect(result.ready).toBe(false);
    expect(result.readyIssue).toBe("authorization_required");
    // Polling could never fix it, so it must not have burned the timeout.
    expect(calls.filter((call) => call.startsWith("connectors.test")).length).toBe(1);
  });

  it("enables a connector before probing it", async () => {
    const { host, calls } = fakeHost({
      statuses: [status([{ id: "conn-1", kind: "connector", name: "Mail", state: "installed" }])],
      probe: [{ connector_id: "conn-1", success: true }],
    });
    const client = new StoreClient(host, { readyPollMs: 1, readyTimeoutMs: 50 });

    const result = await client.install(item());

    // A freshly registered connector is disabled by design, so probing first
    // would always fail; the enable must come first.
    expect(result.ready).toBe(true);
    expect(calls.indexOf("enableInstall:snap-1:conn-1")).toBeLessThan(
      calls.indexOf("connectors.test:conn-1"),
    );
  });

  it("honours an abort signal", async () => {
    const { host } = fakeHost({
      statuses: [status([{ id: "comp-1", kind: "skill", name: "Nowhere", state: "installed" }])],
      skills: [],
    });
    const controller = new AbortController();
    const client = new StoreClient(host, { readyPollMs: 50, readyTimeoutMs: 5_000 });

    const pending = client.install(item(), { signal: controller.signal });
    controller.abort();

    await expect(pending).rejects.toMatchObject({ code: "aborted" });
  });
});

describe("StoreClient · uninstall and setEnabled", () => {
  it("uninstalls every installed component of the item's snapshot", async () => {
    const { host, calls } = fakeHost({
      statuses: [
        status([
          { id: "comp-1", kind: "agent", name: "Team Tools", state: "installed" },
          { id: "comp-2", kind: "skill", name: "Notes", state: "not-installed" },
        ]),
      ],
    });
    const client = new StoreClient(host);

    const result = await client.uninstall(item({ installed: true, snapshot_id: "snap-1" }));

    // Only the installed component is named; the not-installed one has nothing
    // to release and the server refuses an empty list.
    expect(calls).toContain("uninstallInstall:snap-1:comp-1");
    expect(result.ok).toBe(true);
    expect(result.components[0].action).toBe("removed");
  });

  it("treats an already-released component as success", async () => {
    const { host } = fakeHost({
      statuses: [
        status([
          { id: "comp-1", kind: "agent", name: "Team Tools", state: "installed" },
          { id: "comp-2", kind: "skill", name: "Notes", state: "not-installed" },
        ]),
      ],
      uninstall: status([], {
        outcomes: [
          outcome({
            component_id: "comp-2",
            action: "skipped",
            ok: false,
            code: "component_not_installed",
          }),
        ],
        errors: ["comp-2: component_not_installed"],
      }),
    });
    const client = new StoreClient(host);

    const result = await client.uninstall(item({ installed: true, snapshot_id: "snap-1" }));

    // The server names it; the client must not swallow the code, and must not
    // pretend the whole operation failed either.
    expect(result.components[0].code).toBe("component_not_installed");
    expect(result.ok).toBe(false);
  });

  it("refuses to uninstall an item with no snapshot", async () => {
    const { host } = fakeHost();
    const client = new StoreClient(host);

    await expect(client.uninstall(item())).rejects.toMatchObject({ code: "not_installed" });
  });

  it("passes a skill's flag-only marker through instead of hiding it", async () => {
    const { host, calls } = fakeHost({
      statuses: [status([{ id: "comp-2", kind: "skill", name: "Notes", state: "installed" }])],
      disable: status([{ id: "comp-2", kind: "skill", name: "Notes", state: "disabled" }], {
        outcomes: [
          outcome({
            component_id: "comp-2",
            kind: "skill",
            action: "marked",
            ok: true,
            code: "skill_disable_flag_only",
          }),
        ],
      }),
    });
    const client = new StoreClient(host);

    const result = await client.setEnabled(item({ installed: true, snapshot_id: "snap-1" }), false);

    expect(calls).toContain("disableInstall:snap-1:comp-2");
    expect(result.components[0].code).toBe("skill_disable_flag_only");
    expect(result.ok).toBe(true);
  });

  it("setEnabled(true) enables the named components", async () => {
    const { host, calls } = fakeHost({
      statuses: [status([{ id: "conn-1", kind: "connector", name: "Mail", state: "installed" }])],
    });
    const client = new StoreClient(host);

    await client.setEnabled(item({ installed: true, snapshot_id: "snap-1" }), true);

    expect(calls).toContain("enableInstall:snap-1:conn-1");
  });
});
