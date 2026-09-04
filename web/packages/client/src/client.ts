/**
 * Typed App Server client (`@agent-store/client`).
 *
 * Transport-agnostic: all business methods go through the injected
 * `Transport` (07 §2.3). Web-only helpers (asset URLs, `/api/fs/browse`,
 * one-shot HTTP registration) live in the host app, not here.
 *
 * Connection lifecycle: transport connect → `initialize` → protocol version
 * check → `initialized` notification → ready. Business methods require a
 * ready client; the server rejects them with `not_initialized` otherwise.
 */

import { ProtocolError } from "@agent-store/protocol";
import {
  APP_SERVER_PROTOCOL_VERSION,
  type ClientCapabilities,
  type ClientInfo,
  type ImportDetail,
  type ImportRequest,
  type ImportResult,
  type ImportSummary,
  type InstallRequest,
  type InstallResult,
  type InstallStatus,
  type InitializeRequest,
  type InitializeResult,
  type MarketplaceAddRequest,
  type MarketplaceDetail,
  type MarketplaceRefreshResult,
  type MarketplaceRemoveResult,
  type MarketplaceSummary,
  type StoreInstallResult,
  type StoreList,
} from "@agent-store/protocol";
import { AgentClient } from "./agents";
import { ConversationClient } from "./conversations";
import { ConnectorClient } from "./connectors";
import { RunClient } from "./runs";
import { SkillClient } from "./skills";
import { TeamClient } from "./teams";
import { WorkspaceClient } from "./workspaces";
import { type NotificationListener, type Transport } from "./transport";

export interface AppServerClientOptions {
  /** Ready-made transport (07 §2.3); the host app owns its lifecycle. */
  transport: Transport;
  client: ClientInfo;
  capabilities?: ClientCapabilities;
}

export class AppServerClient {
  readonly transport: Transport;
  /** Legacy preset/execution workflow client. */
  readonly runs: RunClient;
  /** Persistent presetless Nomi chat client. */
  readonly conversations: ConversationClient;
  /** Owner-scoped workspace registry client (user-chosen paths). */
  readonly workspaces: WorkspaceClient;
  /** Agent Store Skill catalog client (`skill/list`, `skill/get`). */
  readonly skills: SkillClient;
  /** Agent Store Connector catalog/status/probe/OAuth client. */
  readonly connectors: ConnectorClient;
  /** Agent Store AgentDefinition catalog client (`agent/list`, `agent/get`). */
  readonly agents: AgentClient;
  /** Agent Store Team catalog client (`team/list`, `team/get`). */
  readonly teams: TeamClient;
  readonly clientInfo: ClientInfo;
  readonly capabilities?: ClientCapabilities;

  private initializeResult: InitializeResult | null = null;
  private notificationListeners = new Set<NotificationListener>();

  constructor(options: AppServerClientOptions) {
    this.clientInfo = options.client;
    this.capabilities = options.capabilities;
    this.transport = options.transport;
    this.runs = new RunClient(this.transport);
    this.conversations = new ConversationClient(this.transport);
    this.workspaces = new WorkspaceClient(this.transport);
    this.skills = new SkillClient(this.transport);
    this.connectors = new ConnectorClient(this.transport);
    this.agents = new AgentClient(this.transport);
    this.teams = new TeamClient(this.transport);
    this.transport.onNotification((notification) => {
      for (const listener of [...this.notificationListeners]) {
        try {
          listener(notification);
        } catch {
          // listener isolation
        }
      }
    });
  }

  get ready(): boolean {
    return this.initializeResult !== null;
  }

  get initializeInfo(): InitializeResult | null {
    return this.initializeResult;
  }

  /** Connect and perform the initialize/initialized handshake. */
  async connect(): Promise<InitializeResult> {
    await this.transport.connect();
    const request: InitializeRequest = {
      protocol_version: APP_SERVER_PROTOCOL_VERSION,
      client: this.clientInfo,
      capabilities: this.capabilities,
    };
    const result = await this.transport.request<InitializeResult>("initialize", request);
    if (result.protocol_version !== APP_SERVER_PROTOCOL_VERSION) {
      throw new ProtocolError(
        "version_mismatch",
        `server protocol version ${result.protocol_version} is not supported (client ${APP_SERVER_PROTOCOL_VERSION})`,
      );
    }
    this.transport.notify("initialized", {});
    this.initializeResult = result;
    return result;
  }

  onNotification(listener: NotificationListener): () => void {
    this.notificationListeners.add(listener);
    return () => {
      this.notificationListeners.delete(listener);
    };
  }

  /** Close the transport; the server revokes the connection immediately. */
  close(): void {
    this.transport.close();
    this.initializeResult = null;
    this.notificationListeners.clear();
  }

  /** Import a local CodeBuddy/WorkBuddy source directory (roadmap Phase 1). */
  async runImport(input: ImportRequest): Promise<ImportResult> {
    return this.transport.request<ImportResult>("import/run", input);
  }

  async listImports(): Promise<ImportSummary[]> {
    return this.transport.request<ImportSummary[]>("import/list", {});
  }

  /** One immutable snapshot with its standardized components. */
  async getImport(snapshotId: string): Promise<ImportDetail> {
    return this.transport.request<ImportDetail>("import/get", { snapshot_id: snapshotId });
  }

  /** Install a snapshot into the runtime (roadmap Phase 2). */
  async runInstall(input: InstallRequest): Promise<InstallResult> {
    return this.transport.request<InstallResult>("install/run", input);
  }

  /** Per-component installation state of a snapshot. */
  async getInstallStatus(snapshotId: string): Promise<InstallStatus> {
    return this.transport.request<InstallStatus>("install/status", { snapshot_id: snapshotId });
  }

  /** Disable installed components (runtime artifacts stay). */
  async disableInstall(snapshotId: string, componentIds: string[]): Promise<InstallStatus> {
    return this.transport.request<InstallStatus>("install/disable", {
      snapshot_id: snapshotId,
      component_ids: componentIds,
    });
  }

  /** Re-enable previously disabled components. */
  async enableInstall(snapshotId: string, componentIds: string[]): Promise<InstallStatus> {
    return this.transport.request<InstallStatus>("install/enable", {
      snapshot_id: snapshotId,
      component_ids: componentIds,
    });
  }

  /** Uninstall: remove runtime artifacts and clear the state (snapshot kept). */
  async uninstallInstall(snapshotId: string, componentIds: string[]): Promise<InstallStatus> {
    return this.transport.request<InstallStatus>("install/uninstall", {
      snapshot_id: snapshotId,
      component_ids: componentIds,
    });
  }

  // --- Marketplace (roadmap Phase 2) ---------------------------------------

  /** Register a marketplace source on the trusted host. */
  async addMarketplace(input: MarketplaceAddRequest): Promise<MarketplaceSummary> {
    return this.transport.request<MarketplaceSummary>("market/add", input);
  }

  /** All active marketplaces. */
  async listMarketplaces(): Promise<MarketplaceSummary[]> {
    return this.transport.request<MarketplaceSummary[]>("market/list", {});
  }

  /** One marketplace with its discovered entries. */
  async getMarketplace(marketplaceId: string): Promise<MarketplaceDetail> {
    return this.transport.request<MarketplaceDetail>("market/get", {
      marketplace_id: marketplaceId,
    });
  }

  /** Remove a marketplace (`cascade` uninstalls snapshots installed from it). */
  async removeMarketplace(
    marketplaceId: string,
    cascade: boolean,
  ): Promise<MarketplaceRemoveResult> {
    return this.transport.request<MarketplaceRemoveResult>("market/remove", {
      marketplace_id: marketplaceId,
      cascade,
    });
  }

  /** Toggle auto-update for a marketplace. */
  async setMarketplaceAutoUpdate(
    marketplaceId: string,
    enabled: boolean,
  ): Promise<MarketplaceSummary> {
    return this.transport.request<MarketplaceSummary>("market/auto-update", {
      marketplace_id: marketplaceId,
      enabled,
    });
  }

  /** Re-fetch a marketplace source and rebuild entries when the revision changed. */
  async refreshMarketplace(
    marketplaceId: string,
  ): Promise<MarketplaceRefreshResult> {
    return this.transport.request<MarketplaceRefreshResult>("market/refresh", {
      marketplace_id: marketplaceId,
    });
  }

  /** Import one discovered marketplace entry (provenance linked). */
  async importMarketplaceEntry(
    marketplaceId: string,
    entryName: string,
  ): Promise<ImportResult> {
    return this.transport.request<ImportResult>("market/entry-import", {
      marketplace_id: marketplaceId,
      entry_name: entryName,
    });
  }

  // --- Store (winget-style unified catalog) --------------------------------

  /** All store items across enabled marketplaces (display + install state). */
  async listStore(): Promise<StoreList> {
    return this.transport.request<StoreList>("store/list", {});
  }

  /** One-click install: import (when missing) + register in one call. */
  async installStoreEntry(
    marketplaceId: string,
    entryName: string,
  ): Promise<StoreInstallResult> {
    return this.transport.request<StoreInstallResult>("store/install-entry", {
      marketplace_id: marketplaceId,
      entry_name: entryName,
    });
  }
}