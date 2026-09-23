/**
 * `store` sub-client: the Agent Store lifecycle as one state machine over the
 * flat protocol methods.
 *
 * `AppServerClient`'s top-level methods mirror the wire one-to-one
 * (`runInstall`, `uninstallInstall`, `disableInstall`, ...). That is the right
 * shape for a projection layer and the wrong shape for the question a caller
 * actually has — "install this store item and tell me when it is usable". This
 * class answers that question **without inventing any wire method**: everything
 * below composes methods the protocol already has.
 *
 * Two rules it keeps:
 *
 * - **It never reports a state the server did not report.** Per-component
 *   detail is passed through verbatim, and the readiness verdict is a *separate*
 *   field, so "the server failed a component" and "the client gave up waiting"
 *   can never be confused for one another.
 * - **It never hides a documented asymmetry.** A connector is registered
 *   *disabled* (the installer's documented default), so `waitForReady` switches
 *   it on before probing; and a connector that needs authorization returns
 *   immediately with `authorization_required` rather than burning the timeout.
 */

import type {
  AgentDetail,
  ConnectorProbeResult,
  ConnectorStatusView,
  InstallComponent,
  InstallOutcome,
  InstallStatus,
  SkillSummary,
  StoreInstallResult,
  StoreItem,
  StoreItemKind,
  StoreList,
  TeamDetail,
} from "@flowy-agent-store/protocol";

/** Why a `waitForReady` check ended without every component being usable. */
export type StoreReadyIssue =
  /** The host never reported the component ready inside the timeout. */
  | "ready_timeout"
  /** A connector needs the OAuth browser flow; the caller must drive it. */
  | "authorization_required";

/** Branchable client-side failures. Server-side failures arrive as outcomes. */
export type StoreErrorCode =
  | "not_installed"
  | "item_blocked"
  | "aborted";

export class StoreError extends Error {
  constructor(
    readonly code: StoreErrorCode,
    message: string,
  ) {
    super(message);
    this.name = "StoreError";
  }
}

export interface InstallOptions {
  /**
   * Wait until every component is actually usable before resolving.
   *
   * Defaults to `true` because a resolved `install()` that is not yet usable is
   * the single most misleading thing this surface could do: a skill is usable
   * as soon as it is copied, but a connector is not usable until the server has
   * been switched on and probed.
   */
  waitForReady?: boolean;
  /** Readiness budget. Defaults to `StoreClientOptions.readyTimeoutMs`. */
  timeoutMs?: number;
  signal?: AbortSignal;
}

export interface UninstallOptions {
  /** Narrow the release to these component ids (default: every installed one). */
  componentIds?: string[];
}

export interface SetEnabledOptions {
  /** Narrow the state change to these component ids (default: all installed). */
  componentIds?: string[];
}

export interface StoreClientOptions {
  /** Default readiness budget in ms. */
  readyTimeoutMs?: number;
  /** First readiness poll interval in ms; doubles up to `readyPollMaxMs`. */
  readyPollMs?: number;
  readyPollMaxMs?: number;
  /**
   * Minimum gap between two *real probes* of the same connector while waiting
   * for readiness, in ms. Defaults to `DEFAULT_READY_PROBE_MS`.
   *
   * A connector's readiness cannot be read: the status is derived from the last
   * probe, so someone has to run one — but a probe is a real connection
   * (`initialize` + `tools/list`) that also resolves the stored token, which
   * means a refresh request when that token is near expiry, plus one more on a
   * 401. Probing once per poll (the poll starts at `readyPollMs` = 400ms) turned
   * a 30s readiness budget into ~10 connections and up to ~20 token requests
   * against the connector's authorization server — with nobody having asked for
   * anything. Between probes the loop reads the (free, local) status instead.
   */
  readyProbeMs?: number;
}

export interface StoreOperationOutcome {
  /** The snapshot the operation acted on; `null` when nothing was imported. */
  snapshotId: string | null;
  /** The server reported there was nothing to do. */
  reused: boolean;
  /**
   * The server's per-component detail, verbatim. Empty on a host that predates
   * `outcomes` — treat that as "no detail available", never as "nothing ran".
   */
  components: InstallOutcome[];
  /** Every reported component reached the requested state. */
  ok: boolean;
  /**
   * `undefined` when readiness was not checked. `true` when every component is
   * usable now; `false` when it is not (see `readyIssue`).
   */
  ready?: boolean;
  readyIssue?: StoreReadyIssue;
  /** The component that needs attention, when `readyIssue` is set. */
  readyComponentId?: string;
}

/**
 * The slice of `AppServerClient` this layer orchestrates.
 *
 * Declared structurally rather than imported so this module does not depend on
 * the client that owns it (the client imports this one). It also documents the
 * exact surface the store layer is allowed to lean on.
 */
export interface StoreHost {
  listStore(): Promise<StoreList>;
  installStoreEntry(marketplaceId: string, entryName: string): Promise<StoreInstallResult>;
  getInstallStatus(snapshotId: string): Promise<InstallStatus>;
  disableInstall(snapshotId: string, componentIds: string[]): Promise<InstallStatus>;
  enableInstall(snapshotId: string, componentIds: string[]): Promise<InstallStatus>;
  uninstallInstall(snapshotId: string, componentIds: string[]): Promise<InstallStatus>;
  readonly skills: { list(): Promise<SkillSummary[]> };
  readonly agents: { get(id: string): Promise<AgentDetail> };
  readonly teams: { get(id: string): Promise<TeamDetail> };
  readonly connectors: {
    status(id: string): Promise<ConnectorStatusView>;
    test(id: string): Promise<ConnectorProbeResult>;
  };
}

const DEFAULT_READY_TIMEOUT_MS = 30_000;
const DEFAULT_READY_POLL_MS = 400;
const DEFAULT_READY_POLL_MAX_MS = 4_000;
const DEFAULT_READY_PROBE_MS = 5_000;

/** Every string a `LocalizedText` carries, for haystack building. */
function localizedValues(text: unknown): string[] {
  if (typeof text === "string") return [text];
  if (!text || typeof text !== "object") return [];
  return Object.values(text as Record<string, unknown>).filter(
    (value): value is string => typeof value === "string",
  );
}

function haystack(item: StoreItem): string {
  return [
    item.entry_name,
    item.name,
    item.id,
    item.description ?? "",
    item.version,
    ...localizedValues(item.display_name),
    ...localizedValues(item.display_description),
    ...localizedValues(item.profession),
    ...(item.tags ?? []).flatMap(localizedValues),
  ]
    .join("\n")
    .toLowerCase();
}

function abortableDelay(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) {
      reject(new StoreError("aborted", "the operation was aborted"));
      return;
    }
    const timer = setTimeout(() => {
      signal?.removeEventListener("abort", onAbort);
      resolve();
    }, ms);
    const onAbort = () => {
      clearTimeout(timer);
      reject(new StoreError("aborted", "the operation was aborted"));
    };
    signal?.addEventListener("abort", onAbort, { once: true });
  });
}

/** How one component's readiness check came out. */
type ReadinessVerdict =
  | { ready: true }
  /** `terminal` means polling cannot help — report the issue now. */
  | { ready: false; terminal?: StoreReadyIssue; detail?: string };

export class StoreClient {
  private readonly readyTimeoutMs: number;
  private readonly readyPollMs: number;
  private readonly readyPollMaxMs: number;
  private readonly readyProbeMs: number;

  constructor(
    private readonly host: StoreHost,
    options: StoreClientOptions = {},
  ) {
    this.readyTimeoutMs = options.readyTimeoutMs ?? DEFAULT_READY_TIMEOUT_MS;
    this.readyPollMs = options.readyPollMs ?? DEFAULT_READY_POLL_MS;
    this.readyPollMaxMs = options.readyPollMaxMs ?? DEFAULT_READY_POLL_MAX_MS;
    this.readyProbeMs = options.readyProbeMs ?? DEFAULT_READY_PROBE_MS;
  }

  /** Every item across enabled marketplaces, with install state. */
  async list(): Promise<StoreItem[]> {
    const store = await this.host.listStore();
    return store.items;
  }

  /** Case-insensitive substring search over the fields a user can see. */
  async search(query: string, filter: { kind?: StoreItemKind } = {}): Promise<StoreItem[]> {
    const needle = query.trim().toLowerCase();
    const items = await this.list();
    return items.filter((item) => {
      if (filter.kind && item.kind !== filter.kind) return false;
      if (!needle) return true;
      return haystack(item).includes(needle);
    });
  }

  /** Items currently installed on this host. */
  async installed(): Promise<StoreItem[]> {
    const items = await this.list();
    return items.filter((item) => item.installed);
  }

  /**
   * Installed items whose marketplace offers a different version.
   *
   * Both conditions matter: `update_available` is computed from the imported
   * snapshot even when nothing is installed, so filtering on it alone would
   * report an "update" for an entry the user has never installed.
   */
  async checkUpdates(): Promise<StoreItem[]> {
    const items = await this.list();
    return items.filter((item) => item.installed && item.update_available);
  }

  /**
   * What a caller can offer for an item's pending update.
   *
   * `"none"` means there is nothing to do; `"uninstall_reinstall"` means the
   * entry is installed at a different version and the only path the wire offers
   * is to release it and install again (the version-aware re-import then picks
   * up the current version). `"unknown"` is returned rather than guessing when
   * the item carries no snapshot to release.
   */
  updateHint(item: StoreItem): "none" | "uninstall_reinstall" | "unknown" {
    if (!item.installed || !item.update_available) return "none";
    return item.snapshot_id ? "uninstall_reinstall" : "unknown";
  }

  /**
   * Import (when needed) and install one store item, then wait until it is
   * usable.
   */
  async install(item: StoreItem, options: InstallOptions = {}): Promise<StoreOperationOutcome> {
    this.assertUsable(item, options.signal);
    const result = await this.host.installStoreEntry(item.marketplace_id, item.entry_name);
    const components = result.outcomes ?? [];
    const outcome: StoreOperationOutcome = {
      snapshotId: result.snapshot_id,
      reused: result.reused,
      components,
      ok: result.errors.length === 0 && components.every((component) => component.ok),
    };
    if (options.waitForReady === false) return outcome;
    return this.awaitReady(outcome, options);
  }

  /**
   * Release an item's runtime artifacts and clear its install record.
   *
   * The server only clears the record for components it actually released, so a
   * partial failure leaves `ok: false` with the failing component named — and
   * the item stays installed, which is what makes a retry meaningful.
   */
  async uninstall(item: StoreItem, options: UninstallOptions = {}): Promise<StoreOperationOutcome> {
    const snapshotId = this.requireSnapshot(item);
    const componentIds = options.componentIds ?? (await this.installedComponentIds(snapshotId));
    if (componentIds.length === 0) {
      return { snapshotId, reused: true, components: [], ok: true };
    }
    const status = await this.host.uninstallInstall(snapshotId, componentIds);
    return this.outcomeFromStatus(snapshotId, status, false);
  }

  /**
   * Switch an item's components on or off.
   *
   * Idempotent: the server sets the state it is asked for instead of toggling.
   * A skill reports `skill_disable_flag_only` because the skill corpus has no
   * enable state — that code is passed through, not swallowed, so a caller can
   * tell "switched off" from "marked in the catalogue".
   */
  async setEnabled(
    item: StoreItem,
    enabled: boolean,
    options: SetEnabledOptions = {},
  ): Promise<StoreOperationOutcome> {
    const snapshotId = this.requireSnapshot(item);
    const componentIds = options.componentIds ?? (await this.installedComponentIds(snapshotId));
    if (componentIds.length === 0) {
      return { snapshotId, reused: true, components: [], ok: true };
    }
    const status = enabled
      ? await this.host.enableInstall(snapshotId, componentIds)
      : await this.host.disableInstall(snapshotId, componentIds);
    return this.outcomeFromStatus(snapshotId, status, false);
  }

  private assertUsable(item: StoreItem, signal?: AbortSignal): void {
    if (signal?.aborted) {
      throw new StoreError("aborted", "the operation was aborted before it started");
    }
    if (item.blocked_reason) {
      // The server refuses this entry too (`02` §11.1); failing here with the
      // reason is the same answer, sooner, and without a round trip.
      throw new StoreError("item_blocked", item.blocked_reason);
    }
  }

  private requireSnapshot(item: StoreItem): string {
    const snapshotId = item.snapshot_id;
    if (!snapshotId) {
      throw new StoreError(
        "not_installed",
        `store item ${item.id} has no imported snapshot to act on`,
      );
    }
    return snapshotId;
  }

  private async installedComponentIds(snapshotId: string): Promise<string[]> {
    const status = await this.host.getInstallStatus(snapshotId);
    return status.components
      .filter((component) => component.state !== "not-installed")
      .map((component) => component.id);
  }

  private outcomeFromStatus(
    snapshotId: string,
    status: InstallStatus,
    reused: boolean,
  ): StoreOperationOutcome {
    const components = status.outcomes ?? [];
    const errors = status.errors ?? [];
    return {
      snapshotId,
      reused,
      components,
      // A host that predates `outcomes`/`errors` reports neither; with nothing
      // to contradict it, the call succeeded.
      ok: errors.length === 0 && components.every((component) => component.ok),
    };
  }

  /**
   * Poll until every component of the snapshot is usable.
   *
   * A timeout does **not** discard the install: the caller gets the successful
   * install plus `ready: false` and `readyIssue: "ready_timeout"`, so a slow
   * connector never turns into "the install failed".
   */
  private async awaitReady(
    outcome: StoreOperationOutcome,
    options: InstallOptions,
  ): Promise<StoreOperationOutcome> {
    const snapshotId = outcome.snapshotId;
    if (!snapshotId) return outcome;

    // A connector is registered disabled, so asking whether it is ready before
    // switching it on would always time out. `install/enable` is idempotent.
    const status = await this.host.getInstallStatus(snapshotId);
    const connectors = status.components.filter(
      (component) => component.kind === "connector" && component.state !== "not-installed",
    );
    if (connectors.length > 0) {
      await this.host.enableInstall(
        snapshotId,
        connectors.map((component) => component.id),
      );
    }

    const deadline = Date.now() + (options.timeoutMs ?? this.readyTimeoutMs);
    let interval = this.readyPollMs;
    // The first round always probes (nothing is known yet); after that a
    // connector is re-probed at most once per `readyProbeMs`.
    let nextProbeAt = 0;
    let last: InstallComponent[] = status.components;
    for (;;) {
      const now = Date.now();
      const allowProbe = now >= nextProbeAt;
      if (allowProbe) nextProbeAt = now + this.readyProbeMs;
      const verdicts = await Promise.all(
        last
          .filter((component) => component.state !== "not-installed")
          .map(async (component) => ({
            component,
            verdict: await this.readiness(component, allowProbe),
          })),
      );
      const blocked = verdicts.find((entry) => !entry.verdict.ready);
      if (!blocked) {
        return { ...outcome, ready: true };
      }
      if (!blocked.verdict.ready && blocked.verdict.terminal) {
        return {
          ...outcome,
          ready: false,
          readyIssue: blocked.verdict.terminal,
          readyComponentId: blocked.component.id,
        };
      }
      if (Date.now() >= deadline) {
        return {
          ...outcome,
          ready: false,
          readyIssue: "ready_timeout",
          readyComponentId: blocked.component.id,
        };
      }
      await abortableDelay(interval, options.signal);
      interval = Math.min(interval * 2, this.readyPollMaxMs);
      last = (await this.host.getInstallStatus(snapshotId)).components;
    }
  }

  /**
   * Is this one component usable yet?
   *
   * Each kind answers with the narrowest read the protocol already offers. An
   * unknown kind is reported ready rather than blocking: this layer cannot judge
   * a component it does not understand, and a false timeout on a future kind
   * would be worse than passing it through.
   *
   * `allowProbe` gates the one kind whose readiness needs a real connection (a
   * connector); the other kinds answer from local reads, so the cadence does not
   * apply to them.
   */
  private async readiness(component: InstallComponent, allowProbe: boolean): Promise<ReadinessVerdict> {
    try {
      switch (component.kind) {
        case "skill": {
          const skills = await this.host.skills.list();
          return skills.some((skill) => skill.name === component.name)
            ? { ready: true }
            : { ready: false };
        }
        case "agent": {
          const agent = await this.host.agents.get(component.id);
          return agent.preset_id ? { ready: true } : { ready: false };
        }
        case "team": {
          await this.host.teams.get(component.id);
          return { ready: true };
        }
        case "connector": {
          // Polling the status alone can never *move* a connector: enabling one
          // only flips its stored flag, and the status derives from the *last
          // probe*. Someone has to run a probe, and for a caller waiting on
          // readiness that someone is us — but only every `readyProbeMs`
          // (`allowProbe`): a probe connects for real and resolves the stored
          // token, so probing per poll hammers both the connector and its
          // authorization server. Between probes the status read decides, since
          // it is a local projection of the last probe's outcome.
          if (allowProbe && (await this.host.connectors.test(component.id)).success) {
            return { ready: true };
          }
          const status = await this.host.connectors.status(component.id);
          if (status.status === "connected") return { ready: true };
          if (
            status.status === "authorization_required" ||
            status.status === "reauthorization_required"
          ) {
            // Polling cannot fix this: a human has to finish the OAuth flow.
            return { ready: false, terminal: "authorization_required" };
          }
          return { ready: false };
        }
        default:
          return { ready: true };
      }
    } catch {
      // A not-yet-visible component surfaces as a not-found; keep polling.
      return { ready: false };
    }
  }
}
