/**
 * Agent Store catalog view — marketplace-style card surface.
 *
 * A pure protocol consumer over the App Server link (`src/lib/client.ts`).
 * Four catalog tabs (skills / connectors / agents / teams) plus the importer
 * list render as market cards: circular initial badges, two-line clamps,
 * at most two metadata tags and a right-side detail drawer. Every value is
 * real protocol data; nothing is fabricated by the client.
 *
 * Capabilities are server-driven: tabs only appear when `initialize`
 * advertised them.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowLeft,
  Check,
  ChevronRight,
  CircleAlert,
  LoaderCircle,
  Plus,
  RefreshCw,
  Search,
  Upload,
  Users,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { formatError, isRetryableError } from "../lib/errors";
import { useAppStore } from "../store/appStore";
import type {
  AgentDetail,
  AgentSummary,
  CompatibilityTriple,
  ConnectorDetail,
  ConnectorStatus,
  ConnectorSummary,
  ImportDetail,
  ImportResult,
  ImportSourceKind,
  ImportSummary,
  InstallResult,
  InstallState,
  InstallStatus,
  MarketplaceDetail,
  MarketplaceSourceKind,
  MarketplaceSummary,
  SkillDetail,
  SkillSummary,
  StoreInstallResult,
  StoreItem,
  StoreItemKind,
  StoreList,
  TeamDetail,
  TeamSummary,
} from "../lib/protocol";

type CatalogTab = "store" | "sources" | "installed" | "imports";
type PanelKind = "store" | "skill" | "connector" | "agent" | "team" | "import";
type InstalledKind = "skills" | "connectors" | "agents" | "teams";

// ---------------------------------------------------------------------------
// semantic helpers
// ---------------------------------------------------------------------------

const CONNECTOR_STATE_KEYS: Record<string, string> = {
  installed: "catalog.stateInstalled",
  configured: "catalog.stateConfigured",
  authorization_required: "catalog.stateAuthRequired",
  authenticated: "catalog.stateAuthenticated",
  connected: "catalog.stateConnected",
  degraded: "catalog.stateDegraded",
  error: "catalog.stateError",
  reauthorization_required: "catalog.stateReauthRequired",
};

const AUTH_STATE_KEYS: Record<string, string> = {
  authenticated: "catalog.authAuthenticated",
  not_authenticated: "catalog.authNotAuthenticated",
  reauthorization_required: "catalog.authReauthRequired",
};

const IMPORT_STATUS_KEYS: Record<string, string> = {
  completed: "catalog.importStatusCompleted",
  "completed-with-warnings": "catalog.importStatusWarnings",
  blocked: "catalog.importStatusBlocked",
  failed: "catalog.importStatusFailed",
};

const SEMANTIC_KEYS: Record<string, string> = {
  compatible: "catalog.semantic_compatible",
  compatible_with_adapter: "catalog.semantic_compatible_with_adapter",
  manual_review: "catalog.semantic_manual_review",
  unsupported: "catalog.semantic_unsupported",
  pending_legal_review: "catalog.semantic_pending_legal_review",
};

/** Deterministic badge hues so the same item keeps its color across renders. */
const BADGE_HUES = [
  "#3b6ea5", "#2f7d5a", "#8a5a2e", "#5b5f7d", "#9a3f6b", "#4d7f8a",
];

function badgeColor(seed: string): string {
  let hash = 0;
  for (const char of seed) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return BADGE_HUES[hash % BADGE_HUES.length];
}

function connectorStateClass(status: ConnectorStatus | string): string {
  switch (status) {
    case "connected":
      return "is-success";
    case "error":
    case "degraded":
      return "is-error";
    case "authorization_required":
    case "reauthorization_required":
      return "is-warn";
    default:
      return "";
  }
}

function importStatusClass(status: string): string {
  switch (status) {
    case "completed":
      return "is-success";
    case "completed-with-warnings":
      return "is-warn";
    case "blocked":
    case "failed":
      return "is-error";
    default:
      return "";
  }
}

function stateLabel(
  t: (key: string, opts?: Record<string, unknown>) => string,
  keys: Record<string, string>,
  value: string,
): string {
  const key = keys[value];
  return key ? t(key) : t("catalog.stateOther", { status: value });
}

function semanticLabel(t: (key: string, opts?: Record<string, unknown>) => string, value: string): string {
  const key = SEMANTIC_KEYS[value];
  return key ? t(key) : value;
}

/** Icon to the left of each card row. */
function InitialBadge({ name, size = 40 }: { name: string; size?: number }) {
  const initial = (name.trim()[0] ?? "?").toUpperCase();
  return (
    <span
      className="market-badge"
      style={{ width: size, height: size, background: badgeColor(name), fontSize: size * 0.4 }}
      aria-hidden="true"
    >
      {initial}
    </span>
  );
}

/** Market display name for an agent (localized when present). */
function agentDisplayName(agent: AgentSummary): string {
  return agent.display_name?.zh || agent.display_name?.en || agent.name;
}

/** Badge with the agent avatar when declared, else the initial. */
function AgentBadge({ agent, size = 40 }: { agent: AgentSummary; size?: number }) {
  const name = agentDisplayName(agent);
  if (agent.avatar_url) {
    return (
      <img className="market-badge market-avatar" width={size} height={size} src={agent.avatar_url} alt={name} loading="lazy" />
    );
  }
  return <InitialBadge name={name} size={size} />;
}

/** Store item tag strings (wire may omit empty arrays). */
/** Store item badge: avatar when declared, else the initial. */
function StoreBadge({ item, size = 40, rootUrl }: { item: StoreItem; size?: number; rootUrl?: string }) {
  const name = item.display_name?.zh || item.display_name?.en || item.name;
  const avatar = item.avatar_url
    ? item.avatar_url.startsWith("http://") || item.avatar_url.startsWith("https://")
      ? item.avatar_url
      : rootUrl
        ? `${rootUrl}${item.avatar_url.startsWith("/") ? "" : "/"}${item.avatar_url}`
        : item.avatar_url
    : null;
  if (avatar) {
    return (
      <img
        className="market-badge market-avatar"
        width={size}
        height={size}
        src={avatar}
        alt={name}
        loading="lazy"
      />
    );
  }
  return <InitialBadge name={name} size={size} />;
}

/** Install / installed / update action attached to a store card. */
function StoreInstallAction({
  item,
  busy,
  onInstall,
}: {
  item: StoreItem;
  busy: boolean;
  onInstall: () => void;
}) {
  const { t } = useTranslation();
  if (item.installed) {
    return (
      <button
        className="market-store-float is-installed"
        type="button"
        disabled={busy}
        onClick={onInstall}
        title={item.update_available ? t("catalog.storeUpdate") : t("catalog.storeInstalled")}
        aria-label={item.update_available ? t("catalog.storeUpdate") : t("catalog.storeInstalled")}
      >
        {item.update_available ? <RefreshCw size={14} strokeWidth={1.7} /> : <Check size={14} strokeWidth={2} />}
      </button>
    );
  }
  return (
    <button
      className="market-store-float"
      type="button"
      disabled={busy}
      onClick={onInstall}
      title={busy ? t("catalog.storeInstalling") : t("catalog.storeInstall")}
      aria-label={busy ? t("catalog.storeInstalling") : t("catalog.storeInstall")}
    >
      {busy ? <LoaderCircle className="is-spinning" size={14} strokeWidth={1.7} /> : <Plus size={14} strokeWidth={1.7} />}
    </button>
  );
}

function Tags({ tags }: { tags: string[] }) {
  const { t } = useTranslation();
  const visible = tags.slice(0, 2);
  const extra = tags.length - visible.length;
  return (
    <div className="market-tags">
      {visible.map((tag) => (
        <span className="market-tag" key={tag}>{tag}</span>
      ))}
      {extra > 0 && <span className="market-tag is-extra">+{extra}</span>}
    </div>
  );
}

function CompatChips({ triple }: { triple: CompatibilityTriple }) {
  const { t } = useTranslation();
  if (!triple) return null;
  return (
    <div className="market-compat">
      <span className="market-tag">{semanticLabel(t, triple.semantic_status)}</span>
      <span className="market-tag is-muted">{t("catalog.compatRuntime")}: {triple.runtime_status}</span>
      <span className="market-tag is-muted">{t("catalog.compatDistribution")}: {triple.distribution_status}</span>
      {triple.reasons.length > 0 && (
        <details className="market-reasons">
          <summary>{t("catalog.compatReasons")}</summary>
          <ul>{triple.reasons.map((reason, index) => <li key={index}>{reason}</li>)}</ul>
        </details>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Catalog view
// ---------------------------------------------------------------------------

export function CatalogView() {
  const { t } = useTranslation();
  const client = useAppStore((s) => s.client);
  const capabilities = useAppStore((s) => s.client?.initializeInfo?.capabilities ?? null);
  const onBack = useAppStore((s) => s.toggleCatalog);

  const [tab, setTab] = useState<CatalogTab>("store");
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState<string>("all");
  const [installedKind, setInstalledKind] = useState<InstalledKind>("agents");

  // store (winget-style aggregated catalog)
  const [storeItems, setStoreItems] = useState<StoreItem[] | null>(null);
  const [storeKind, setStoreKind] = useState<"all" | StoreItemKind>("all");
  const [sortBy, setSortBy] = useState<"default" | "name">("default");
  const [storeInstallBusy, setStoreInstallBusy] = useState<string | null>(null);

  // lists (null = loading)
  const [skills, setSkills] = useState<SkillSummary[] | null>(null);
  const [connectors, setConnectors] = useState<ConnectorSummary[] | null>(null);
  const [agents, setAgents] = useState<AgentSummary[] | null>(null);
  const [teams, setTeams] = useState<TeamSummary[] | null>(null);
  const [imports, setImports] = useState<ImportSummary[] | null>(null);
  const [markets, setMarkets] = useState<MarketplaceSummary[] | null>(null);

  // detail drawer
  const [drawer, setDrawer] = useState<PanelKind | null>(null);
  const [skillDetail, setSkillDetail] = useState<SkillDetail | null>(null);
  const [connectorDetail, setConnectorDetail] = useState<ConnectorDetail | null>(null);
  const [agentDetail, setAgentDetail] = useState<AgentDetail | null>(null);
  const [teamDetail, setTeamDetail] = useState<TeamDetail | null>(null);
  const [importDetail, setImportDetail] = useState<ImportDetail | null>(null);
  const [installStatus, setInstallStatus] = useState<InstallStatus | null>(null);
  const [installResult, setInstallResult] = useState<InstallResult | null>(null);
  const [detailBusy, setDetailBusy] = useState<string | null>(null);

  // store drawer (winget-style item detail + install)
  const [storeDrawerItem, setStoreDrawerItem] = useState<StoreItem | null>(null);
  const [storeInstallResult, setStoreInstallResult] = useState<StoreInstallResult | null>(null);

  // connector auth state map (id -> oauth state)
  const [authMap, setAuthMap] = useState<Map<string, string>>(new Map());
  const [error, setError] = useState<string | null>(null);

  // importer form
  const [importResult, setImportResult] = useState<ImportResult | null>(null);
  const [importPath, setImportPath] = useState("");
  const [importKind, setImportKind] = useState<ImportSourceKind>("codebuddy-plugin");
  const [importBusy, setImportBusy] = useState(false);

  // marketplace panel
  const [marketPath, setMarketPath] = useState("");
  const [marketKind, setMarketKind] = useState<MarketplaceSourceKind>("directory");
  const [marketBusy, setMarketBusy] = useState(false);
  const [marketDetail, setMarketDetail] = useState<MarketplaceDetail | null>(null);
  const [marketRefreshBusy, setMarketRefreshBusy] = useState<string | null>(null);

  const [reloadTick, setReloadTick] = useState(0);
  const activeRef = useRef(true);

  const reportError = (caught: unknown) => {
    setError(formatError(caught) + (isRetryableError(caught) ? t("common.retryable") : ""));
  };

  useEffect(() => {
    activeRef.current = true;
    return () => {
      activeRef.current = false;
    };
  }, []);

  const reload = useCallback(() => {
    setStoreItems(null);
    setSkills(null);
    setConnectors(null);
    setAgents(null);
    setTeams(null);
    setImports(null);
    setMarkets(null);
    setError(null);
    setReloadTick((tick) => tick + 1);
  }, []);

  // One effect loads all catalog slices; a single slice failing (notably the
  // HTTP-backed imports channel) must never blank the rest.
  useEffect(() => {
    if (!client) return;
    let cancelled = false;
    const load = async () => {
      try {
        const [storeList, skillList, connectorList, agentList, teamList, importList, marketList] = await Promise.all([
          capabilities?.store ? client.listStore().catch(() => null) : Promise.resolve(null),
          capabilities?.skills ? client.skills.list().catch(() => []) : Promise.resolve([]),
          capabilities?.connectors ? client.connectors.list().catch(() => []) : Promise.resolve([]),
          capabilities?.agents ? client.agents.list().catch(() => []) : Promise.resolve([]),
          capabilities?.teams ? client.teams.list().catch(() => []) : Promise.resolve([]),
          capabilities?.imports ? client.listImports().catch(() => []) : Promise.resolve([]),
          capabilities?.marketplaces ? client.listMarketplaces().catch(() => []) : Promise.resolve([]),
        ]);
        if (cancelled || !activeRef.current) return;
        setStoreItems(storeList?.items ?? null);
        setSkills(skillList);
        setConnectors(connectorList);
        setAgents(agentList);
        setTeams(teamList);
        setImports(importList);
        setMarkets(marketList);
      } catch (caught) {
        if (cancelled || !activeRef.current) return;
        reportError(caught);
        setStoreItems(null);
        setSkills(null);
        setConnectors(null);
        setAgents(null);
        setTeams(null);
        setImports(null);
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, [client, capabilities, reloadTick]);

  const openSkill = useCallback(async (skillId: string) => {
    if (!client) return;
    setDrawer("skill");
    setDetailBusy(skillId);
    setSkillDetail(null);
    try {
      const detail = await client.skills.get(skillId);
      if (!activeRef.current) return;
      setSkillDetail(detail);
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
    } finally {
      if (activeRef.current) setDetailBusy(null);
    }
  }, [client]);

  const openConnector = useCallback(async (connectorId: string) => {
    if (!client) return;
    setDrawer("connector");
    setDetailBusy(connectorId);
    setConnectorDetail(null);
    try {
      const [detail, status] = await Promise.all([
        client.connectors.get(connectorId),
        client.connectors.status(connectorId),
      ]);
      if (!activeRef.current) return;
      setConnectorDetail(detail);
      if (status.auth_status?.state) {
        setAuthMap((map) => new Map(map).set(connectorId, status.auth_status?.state ?? "not_authenticated"));
      }
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
    } finally {
      if (activeRef.current) setDetailBusy(null);
    }
  }, [client]);

  const probeConnector = useCallback(async (connectorId: string) => {
    if (!client) return;
    setDetailBusy(connectorId);
    try {
      const result = await client.connectors.test(connectorId);
      if (!activeRef.current) return;
      setError(result.success ? null : t("catalog.probeFailed", { error: result.error ?? result.code ?? t("catalog.unknownError") }));
      const [detail, status] = await Promise.all([
        client.connectors.get(connectorId),
        client.connectors.status(connectorId),
      ]);
      if (!activeRef.current) return;
      setConnectorDetail(detail);
      if (status.auth_status?.state) {
        setAuthMap((map) => new Map(map).set(connectorId, status.auth_status?.state ?? "not_authenticated"));
      }
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
    } finally {
      if (activeRef.current) setDetailBusy(null);
    }
  }, [client, t]);

  const refreshAuth = useCallback(async (connectorId: string) => {
    if (!client) return;
    try {
      const status = await client.connectors.authStatus(connectorId);
      if (!activeRef.current) return;
      setAuthMap((map) => new Map(map).set(connectorId, status.state));
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
    }
  }, [client]);

  const startAuth = useCallback(async (connectorId: string) => {
    if (!client) return;
    setDetailBusy(connectorId);
    try {
      const started = await client.connectors.authStart(connectorId);
      if (!activeRef.current) return;
      if (started.error) {
        setError(t("catalog.authStartFailed", { error: started.error }));
        return;
      }
      setError(t("catalog.authStarted"));
      await refreshAuth(connectorId);
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
    } finally {
      if (activeRef.current) setDetailBusy(null);
    }
  }, [client, refreshAuth, t]);

  const logoutConnector = useCallback(async (connectorId: string) => {
    if (!client) return;
    setDetailBusy(connectorId);
    try {
      await client.connectors.logout(connectorId);
      if (!activeRef.current) return;
      setAuthMap((map) => new Map(map).set(connectorId, "not_authenticated"));
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
    } finally {
      if (activeRef.current) setDetailBusy(null);
    }
  }, [client]);

  const openAgent = useCallback(async (agentId: string) => {
    if (!client) return;
    setDrawer("agent");
    setDetailBusy(agentId);
    setAgentDetail(null);
    try {
      const detail = await client.agents.get(agentId);
      if (!activeRef.current) return;
      setAgentDetail(detail);
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
    } finally {
      if (activeRef.current) setDetailBusy(null);
    }
  }, [client]);

  const openTeam = useCallback(async (teamId: string) => {
    if (!client) return;
    setDrawer("team");
    setDetailBusy(teamId);
    setTeamDetail(null);
    try {
      const detail = await client.teams.get(teamId);
      if (!activeRef.current) return;
      setTeamDetail(detail);
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
    } finally {
      if (activeRef.current) setDetailBusy(null);
    }
  }, [client]);

  const openImport = useCallback(async (snapshotId: string) => {
    if (!client) return;
    setDrawer("import");
    setDetailBusy(snapshotId);
    setImportDetail(null);
    setInstallStatus(null);
    try {
      const detail = await client.getImport(snapshotId);
      if (!activeRef.current) return;
      setImportDetail(detail);
      // Load the installation state alongside the catalog detail (Phase 2).
      try {
        const status = await client.getInstallStatus(snapshotId);
        if (activeRef.current) setInstallStatus(status);
      } catch {
        if (activeRef.current) setInstallStatus(null);
      }
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
    } finally {
      if (activeRef.current) setDetailBusy(null);
    }
  }, [client]);

  const runInstall = useCallback(async (snapshotId: string) => {
    if (!client) return;
    setDetailBusy(snapshotId);
    try {
      const result = await client.runInstall({ snapshot_id: snapshotId });
      if (!activeRef.current) return;
      setInstallResult(result);
      const status = await client.getInstallStatus(snapshotId);
      if (activeRef.current) setInstallStatus(status);
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
    } finally {
      if (activeRef.current) setDetailBusy(null);
    }
  }, [client]);

  const toggleInstall = useCallback(async (kind: "enable" | "disable" | "uninstall", snapshotId: string, componentId: string) => {
    if (!client) return;
    setDetailBusy(`${kind}:${componentId}`);
    try {
      const status = kind === "enable"
        ? await client.enableInstall(snapshotId, [componentId])
        : kind === "disable"
          ? await client.disableInstall(snapshotId, [componentId])
          : await client.uninstallInstall(snapshotId, [componentId]);
      if (!activeRef.current) return;
      setInstallStatus(status);
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
    } finally {
      if (activeRef.current) setDetailBusy(null);
    }
  }, [client]);

  const runImport = useCallback(async () => {
    if (!client || importPath.trim().length === 0) {
      setError(t("catalog.importEmptySource"));
      return;
    }
    setImportBusy(true);
    setImportResult(null);
    try {
      const result = await client.runImport({ source_path: importPath.trim(), source_kind: importKind });
      if (!activeRef.current) return;
      setImportResult(result);
      const list = await client.listImports();
      if (!activeRef.current) return;
      setImports(list);
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
    } finally {
      if (activeRef.current) setImportBusy(false);
    }
  }, [client, importPath, importKind, t]);

  const addMarket = useCallback(async () => {
    if (!client) return;
    const path = marketPath.trim();
    if (!path) {
      setError(t("catalog.marketPathRequired"));
      return;
    }
    setMarketBusy(true);
    setError(null);
    try {
      const summary = await client.addMarketplace({ source_kind: marketKind, source: path });
      const list = await client.listMarketplaces();
      if (!activeRef.current) return;
      setMarkets(list);
      setMarketPath("");
      const detail = await client.getMarketplace(summary.marketplace_id);
      if (!activeRef.current) return;
      setMarketDetail(detail);
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
    } finally {
      if (activeRef.current) setMarketBusy(false);
    }
  }, [client, marketPath, marketKind, t]);

  const refreshMarket = useCallback(async (marketplaceId: string) => {
    if (!client) return;
    setMarketRefreshBusy(marketplaceId);
    setError(null);
    try {
      await client.refreshMarketplace(marketplaceId);
      const [list, detail] = await Promise.all([
        client.listMarketplaces(),
        client.getMarketplace(marketplaceId).catch(() => null),
      ]);
      if (!activeRef.current) return;
      setMarkets(list);
      if (detail) setMarketDetail(detail);
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
    } finally {
      if (activeRef.current) setMarketRefreshBusy(null);
    }
  }, [client]);

  const openMarket = useCallback(async (marketplaceId: string) => {
    if (!client) return;
    setDetailBusy(marketplaceId);
    setError(null);
    try {
      const detail = await client.getMarketplace(marketplaceId);
      if (!activeRef.current) return;
      setMarketDetail(detail);
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
    } finally {
      if (activeRef.current) setDetailBusy(null);
    }
  }, [client]);

  const removeMarket = useCallback(async (marketplaceId: string) => {
    if (!client) return;
    const confirmed = window.confirm(t("catalog.marketRemoveConfirm"));
    if (!confirmed) return;
    setMarketBusy(true);
    setError(null);
    try {
      await client.removeMarketplace(marketplaceId, true);
      const [list, importsList] = await Promise.all([client.listMarketplaces(), client.listImports()]);
      if (!activeRef.current) return;
      setMarkets(list);
      setImports(importsList);
      setMarketDetail(null);
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
    } finally {
      if (activeRef.current) setMarketBusy(false);
    }
  }, [client, t]);

  const openStoreItem = useCallback((item: StoreItem) => {
    setDrawer("store");
    setStoreDrawerItem(item);
  }, []);

  const runStoreInstall = useCallback(async (item: StoreItem) => {
    if (!client) return;
    setStoreInstallBusy(item.id);
    setError(null);
    try {
      const result = await client.installStoreEntry(item.marketplace_id, item.entry_name);
      if (!activeRef.current) return;
      setStoreInstallResult(result);
      // Refresh the aggregated store so the installed state flips.
      const list = await client.listStore();
      if (!activeRef.current) return;
      setStoreItems(list.items);
      // Keep the open drawer's item in sync.
      setStoreDrawerItem((current) => {
        if (!current) return current;
        const refreshed = list.items.find(
          (candidate) => candidate.id === current.id,
        );
        return refreshed ?? { ...current, installed: true, snapshot_id: result.snapshot_id, installed_version: result.version };
      });
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
    } finally {
      if (activeRef.current) setStoreInstallBusy(null);
    }
  }, [client]);

  const closeDrawer = useCallback(() => {
    setDrawer(null);
    setStoreDrawerItem(null);
    setStoreInstallResult(null);
    setSkillDetail(null);
    setConnectorDetail(null);
    setAgentDetail(null);
    setTeamDetail(null);
    setImportDetail(null);
    setInstallStatus(null);
  }, []);

  // Escape closes the drawer; click on the mask closes it too.
  useEffect(() => {
    if (!drawer) return;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") closeDrawer();
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [drawer, closeDrawer]);

  // ---------- derived catalog data ----------
  const visibleSkills = useMemo(() => {
    return (skills ?? []).filter((skill) => {
      const text = `${skill.name} ${skill.description ?? ""} ${skill.source}`.toLowerCase();
      return text.includes(query.toLowerCase());
    });
  }, [skills, query]);

  const skillCategories = useMemo(() => {
    return Array.from(new Set((skills ?? []).map((skill) => skill.source))).sort();
  }, [skills]);

  const visibleConnectors = useMemo(() => {
    return (connectors ?? []).filter((connector) => {
      const text = `${connector.name} ${connector.description ?? ""} ${connector.kind} ${connector.auth_mode}`.toLowerCase();
      return text.includes(query.toLowerCase());
    });
  }, [connectors, query]);

  const connectorCategories = useMemo(() => {
    return Array.from(new Set((connectors ?? []).map((connector) => connector.kind))).sort();
  }, [connectors]);

  const visibleAgents = useMemo(() => {
    return (agents ?? []).filter((agent) => {
      const text = `${agent.name} ${agent.description ?? ""} ${agent.source}`.toLowerCase();
      return text.includes(query.toLowerCase());
    });
  }, [agents, query]);

  const agentCategories = useMemo(() => {
    return Array.from(new Set((agents ?? []).map((agent) => agent.source))).sort();
  }, [agents]);

  const visibleTeams = useMemo(() => {
    return (teams ?? []).filter((team) => {
      const text = `${team.name} ${team.description ?? ""} ${team.source}`.toLowerCase();
      return text.includes(query.toLowerCase());
    });
  }, [teams, query]);

  const teamCategories = useMemo(() => {
    return Array.from(new Set((teams ?? []).map((team) => team.source))).sort();
  }, [teams]);

  const visibleImports = useMemo(() => {
    return (imports ?? []).filter((item) => {
      const text = `${item.name} ${item.version} ${item.source_kind} ${item.status}`.toLowerCase();
      return text.includes(query.toLowerCase());
    });
  }, [imports, query]);

  const importsFiltered =
    category === "all" ? visibleImports : visibleImports.filter((item) => item.source_kind === category);

  // Store items: filter by kind chips + search across display metadata.
  const visibleStoreItems = useMemo(() => {
    const all = storeItems ?? [];
    const byKind = storeKind === "all" ? all : all.filter((item) => item.kind === storeKind);
    const needle = query.trim().toLowerCase();
    const filtered = needle ? byKind.filter((item) => {
      const text = [
        item.name,
        item.display_name?.zh ?? "",
        item.display_name?.en ?? "",
        item.profession?.zh ?? "",
        item.profession?.en ?? "",
        item.description ?? "",
        item.display_description?.zh ?? "",
        item.display_description?.en ?? "",
        item.marketplace_name,
        item.entry_name,
      ].join(" ").toLowerCase();
      return text.includes(needle);
    }) : byKind;
    if (sortBy === "name") {
      return [...filtered].sort((a, b) =>
        (a.display_name?.zh || a.display_name?.en || a.name).localeCompare(b.display_name?.zh || b.display_name?.en || b.name, "zh"),
      );
    }
    return filtered;
  }, [storeItems, storeKind, query, sortBy]);

  const storeKinds = useMemo(() => {
    const kinds = new Set<StoreItemKind>();
    (storeItems ?? []).forEach((item) => kinds.add(item.kind));
    return (["agent", "team", "skill", "connector"] as const).filter((kind) =>
      kinds.has(kind),
    );
  }, [storeItems]);

  const connectorDetailAuth = connectorDetail ? authMap.get(connectorDetail.id) : undefined;

  // ---------- segmented tabs (专家 / 技能 / 连接器 / 我的专家) ----------
  // Top tabs removed: kind filtering is done by the category row (storeKind).

  const searchPlaceholder: Record<CatalogTab, string> = {
    store: t("catalog.storeSearch"),
    sources: t("catalog.searchImports"),
    installed: t("catalog.searchAgents"),
    imports: t("catalog.searchImports"),
  };

  return (
    <section className="market-view" aria-label={t("catalog.ariaLabel")}>
      {/* Page header */}
      <header className="market-header">
        <div className="market-header-left">
          <button className="icon-button market-back" type="button" aria-label={t("catalog.backToChat")} title={t("catalog.backToChat")} onClick={onBack}>
            <ArrowLeft size={19} strokeWidth={1.7} />
          </button>
          <div className="market-title">
            <span>{t("catalog.subtitle")}</span>
          </div>
        </div>
        <button className="icon-button" type="button" aria-label={t("catalog.reload")} title={t("catalog.reloadTitle")} onClick={reload}>
          <RefreshCw size={16} strokeWidth={1.7} />
        </button>
      </header>

      {/* Segmented tabs */}
      <div className="market-tabs" role="tablist" aria-label={t("catalog.typeLabel")}>
        {(tab === "store" || tab === "installed") && (
          <label className="market-search market-search-inline">
            <Search size={15} strokeWidth={1.7} />
            <input
              type="search"
              value={query}
              placeholder={searchPlaceholder[tab]}
              onChange={(event) => setQuery(event.target.value)}
            />
          </label>
        )}
        <button
          className="market-mine"
          type="button"
          onClick={() => { setTab("installed"); }}
        >
          <Users size={15} strokeWidth={1.7} />
          <span>{t("catalog.tabInstalled")}</span>
        </button>
      </div>

      {error && (
        <div className="market-alert" role="alert">
          <CircleAlert size={15} strokeWidth={1.8} />
          <span>{error}</span>
          <button type="button" aria-label={t("common.closeError")} onClick={() => setError(null)}>✕</button>
        </div>
      )}

      {/* Toolbar: search + category chips (sources/imports use their own forms) */}
      {tab === "store" && (
        <div className="market-toolbar market-toolbar-store">
          <div className="market-cats" role="group" aria-label={t("catalog.typeLabel")}>
            <button
              className={`market-chip ${storeKind === "all" ? "is-active" : ""}`}
              type="button"
              onClick={() => setStoreKind("all")}
            >
              {t("catalog.storeAll")}
            </button>
            {storeKinds.map((kind) => (
              <button
                key={kind}
                className={`market-chip ${storeKind === kind ? "is-active" : ""}`}
                type="button"
                onClick={() => setStoreKind(kind)}
              >
                {kind === "agent" ? t("catalog.kindAgent") : kind === "team" ? t("catalog.kindTeam") : kind === "skill" ? t("catalog.kindSkill") : t("catalog.kindConnector")}
              </button>
            ))}
          </div>
          <div className="market-sort" role="group" aria-label={t("catalog.sortLabel")}>
            <button
              className={`market-chip market-sort-chip ${sortBy === "default" ? "is-active" : ""}`}
              type="button"
              onClick={() => setSortBy("default")}
            >
              {t("catalog.sortDefault")}
            </button>
            <button
              className={`market-chip market-sort-chip ${sortBy === "name" ? "is-active" : ""}`}
              type="button"
              onClick={() => setSortBy("name")}
            >
              {t("catalog.sortName")}
            </button>
          </div>
        </div>
      )}

      {tab === "installed" && (
        <div className="market-toolbar market-toolbar-store">
          <div className="market-cats" role="group" aria-label={t("catalog.typeLabel")}>
            {([
              { key: "agents" as const, label: t("catalog.kindAgent") },
              { key: "teams" as const, label: t("catalog.kindTeam") },
              { key: "skills" as const, label: t("catalog.kindSkill") },
              { key: "connectors" as const, label: t("catalog.kindConnector") },
            ] as { key: InstalledKind; label: string }[]).filter((entry) =>
              entry.key === "skills" ? capabilities?.skills !== false
                : entry.key === "connectors" ? capabilities?.connectors !== false
                  : entry.key === "agents" ? capabilities?.agents !== false
                    : capabilities?.teams !== false,
            ).map((entry) => (
              <button
                key={entry.key}
                className={`market-chip ${installedKind === entry.key ? "is-active" : ""}`}
                type="button"
                onClick={() => { setInstalledKind(entry.key); setCategory("all"); }}
              >
                {entry.label}
              </button>
            ))}
          </div>
        </div>
      )}

      {/* Body */}
      <div className="market-body">
        {tab === "store" && (
          <MarketGrid
            loading={!client || storeItems === null}
            empty={t("catalog.storeEmpty")}
            emptyAction={(
              <button className="market-empty-action" type="button" onClick={() => setTab("sources")}>
                {t("catalog.emptyGoSources")}
              </button>
            )}
            items={visibleStoreItems}
            renderItem={(item) => (
              <div className="market-card market-store-card" key={item.id}>
                <div
                  className="market-store-main"
                  role="button"
                  tabIndex={0}
                  onClick={() => openStoreItem(item)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      openStoreItem(item);
                    }
                  }}
                >
                  <div className="market-card-top">
                    <StoreBadge item={item} rootUrl={client?.serverRootUrl} />
                    <div className="market-card-main">
                      <span className="market-card-title">{item.display_name?.zh || item.display_name?.en || item.name}</span>
                      {item.profession?.zh || item.profession?.en ? (
                        <span className="market-card-author">{item.profession?.zh || item.profession?.en}</span>
                      ) : null}
                    </div>
                  </div>
                  {(item.display_description?.zh || item.display_description?.en || item.description) && (
                    <span className="market-card-desc">{item.display_description?.zh || item.display_description?.en || item.description}</span>
                  )}
                </div>
                <div className="market-card-foot">
                  <div className="market-tags">
                    {(item.tags ?? []).slice(0, 3).map((tag) => {
                      const text = tag.zh || tag.en || "";
                      return text ? <span className="market-tag" key={text}>{text}</span> : null;
                    })}
                  </div>
                </div>
                <StoreInstallAction
                  item={item}
                  busy={storeInstallBusy === item.id}
                  onInstall={() => void runStoreInstall(item)}
                />
              </div>
            )}
          />
        )}

        {tab === "installed" && installedKind === "skills" && (
          <MarketGrid
            loading={!client || skills === null}
            empty={t("catalog.noSkills")}
            emptyAction={(
              <button className="market-empty-action" type="button" onClick={() => setTab("store")}>
                {t("catalog.emptyGoStore")}
              </button>
            )}
            items={category === "all" ? visibleSkills : visibleSkills.filter((skill) => skill.source === category)}
            renderItem={(skill) => (
              <button className="market-card" type="button" key={skill.id} onClick={() => void openSkill(skill.id)}>
                <div className="market-card-top">
                  <InitialBadge name={skill.name} />
                  <div className="market-card-main">
                    <span className="market-card-title">{skill.name}</span>
                    {skill.description && <span className="market-card-sub">{skill.description}</span>}
                  </div>
                  <ChevronRight size={15} strokeWidth={1.7} className="market-card-arrow" />
                </div>
                <Tags tags={[skill.source, ...(skill.required_connectors ?? [])]} />
              </button>
            )}
          />
        )}

        {tab === "installed" && installedKind === "connectors" && (
          <MarketGrid
            loading={!client || connectors === null}
            empty={t("catalog.noConnectors")}
            emptyAction={(
              <button className="market-empty-action" type="button" onClick={() => setTab("store")}>
                {t("catalog.emptyGoStore")}
              </button>
            )}
            items={category === "all" ? visibleConnectors : visibleConnectors.filter((connector) => connector.kind === category)}
            renderItem={(connector) => (
              <button className="market-card" type="button" key={connector.id} onClick={() => void openConnector(connector.id)}>
                <div className="market-card-top">
                  <InitialBadge name={connector.name} />
                  <div className="market-card-main">
                    <span className="market-card-title">{connector.name}</span>
                    <span className="market-card-sub">{connector.transport_summary}</span>
                  </div>
                  <span className={`status-dot ${connectorStateClass(connector.status)}`} aria-hidden="true" />
                </div>
                <Tags tags={[connector.kind, connector.auth_mode, ...(connector.description ? [connector.description] : [])]} />
              </button>
            )}
          />
        )}

        {tab === "installed" && installedKind === "agents" && (
          <MarketGrid
            loading={!client || agents === null}
            empty={t("catalog.noAgents")}
            emptyAction={(
              <button className="market-empty-action" type="button" onClick={() => setTab("store")}>
                {t("catalog.emptyGoStore")}
              </button>
            )}
            items={category === "all" ? visibleAgents : visibleAgents.filter((agent) => agent.source === category)}
            renderItem={(agent) => (
              <button className="market-card" type="button" key={agent.id} onClick={() => void openAgent(agent.id)}>
                <div className="market-card-top">
                  <AgentBadge agent={agent} />
                  <div className="market-card-main">
                    <span className="market-card-title">{agentDisplayName(agent)}</span>
                    {agent.description && <span className="market-card-sub">{agent.description}</span>}
                  </div>
                  <ChevronRight size={15} strokeWidth={1.7} className="market-card-arrow" />
                </div>
                <Tags tags={[
                  agent.source,
                  ...(agent.model_summary ? [agent.model_summary] : []),
                  ...(agent.tool_policy_summary ? [agent.tool_policy_summary] : []),
                ]} />
              </button>
            )}
          />
        )}

        {tab === "installed" && installedKind === "teams" && (
          <>
            {visibleTeams.length > 0 && (
              <>
                <h2 className="market-section">{t("catalog.featuredScenes")}</h2>
                <div className="market-scenes" role="list">
                  {visibleTeams.map((team) => (
                    <button className="market-scene" type="button" role="listitem" key={team.id} onClick={() => void openTeam(team.id)}>
                      <div className="market-scene-head">
                        <span className="market-scene-title">{team.name}</span>
                        <span className="market-scene-count">{team.member_agent_ids.length + 1} {t("catalog.teamMembers")}</span>
                      </div>
                      <div className="market-scene-body">
                        {team.description ?? t("catalog.noDescription")}
                      </div>
                    </button>
                  ))}
                </div>
              </>
            )}
            {(!client || agents === null || visibleAgents.length > 0) && (
              <h2 className="market-section">{t("catalog.experts")}</h2>
            )}
            <MarketGrid
              loading={!client || agents === null}
              empty={t("catalog.noAgents")}
              emptyAction={(
                <button className="market-empty-action" type="button" onClick={() => setTab("store")}>
                  {t("catalog.emptyGoStore")}
                </button>
              )}
              items={visibleAgents}
              renderItem={(agent) => (
                <button className="market-card" type="button" key={agent.id} onClick={() => void openAgent(agent.id)}>
                  <div className="market-card-top">
                    <AgentBadge agent={agent} />
                    <div className="market-card-main">
                      <span className="market-card-title">{agentDisplayName(agent)}</span>
                      {agent.description && <span className="market-card-sub">{agent.description}</span>}
                    </div>
                    <ChevronRight size={15} strokeWidth={1.7} className="market-card-arrow" />
                  </div>
                  <Tags tags={[agent.source, ...(agent.model_summary ? [agent.model_summary] : [])]} />
                </button>
              )}
            />
          </>
        )}

        {tab === "sources" && (
          <div className="market-imports">
            {/* Marketplace source panel: add (http/git first) + list + detail */}
            <div className="market-market-panel">
              <div className="market-import-form">
                <label htmlFor="market-path">{t("catalog.marketSource")}</label>
                <div className="market-import-row">
                  <select
                    className="market-select"
                    value={marketKind}
                    onChange={(event) => setMarketKind(event.target.value as MarketplaceSourceKind)}
                  >
                    <option value="github">{t("catalog.marketKindGithub")}</option>
                    <option value="git">{t("catalog.marketKindGit")}</option>
                    <option value="url">{t("catalog.marketKindUrl")}</option>
                    <option value="directory">{t("catalog.marketKindDirectory")}</option>
                  </select>
                  <input
                    className="market-input"
                    type="text"
                    id="market-path"
                    placeholder={marketKind === "directory"
                      ? t("catalog.marketPathPlaceholder")
                      : marketKind === "github"
                        ? t("catalog.marketGithubPlaceholder")
                        : marketKind === "git"
                          ? t("catalog.marketGitPlaceholder")
                          : t("catalog.marketUrlPlaceholder")}
                    value={marketPath}
                    onChange={(event) => setMarketPath(event.target.value)}
                    onKeyDown={(event) => { if (event.key === "Enter") void addMarket(); }}
                  />
                  <button className="primary-button" type="button" disabled={marketBusy} onClick={() => void addMarket()}>
                    {marketBusy ? t("catalog.marketAdding") : t("catalog.marketAdd")}
                  </button>
                </div>
              </div>

              {markets === null && <p className="market-empty">{t("catalog.loadingMarkets")}</p>}
              {markets !== null && markets.length === 0 && (
                <p className="market-empty">{t("catalog.noMarkets")}</p>
              )}
              {markets !== null && markets.length > 0 && (
                <div className="market-list">
                  {markets.map((market) => (
                    <button className="market-card" type="button" key={market.marketplace_id}
                      onClick={() => void openMarket(market.marketplace_id)}>
                      <div className="market-card-top">
                        <InitialBadge name={market.name} />
                        <div className="market-card-main">
                          <span className="market-card-title">{market.name}</span>
                          <span className="market-card-sub">
                            {market.source_kind} · {t("catalog.marketEntries", { count: market.entry_count })}
                            {market.auto_update ? ` · ${t("catalog.marketAutoUpdate")}` : ""}
                          </span>
                        </div>
                        <ChevronRight size={15} strokeWidth={1.7} className="market-card-arrow" />
                      </div>
                    </button>
                  ))}
                </div>
              )}

              {marketDetail && (
                <div className="market-market-detail">
                  <div className="market-market-detail-head">
                    <div>
                      <h3>{marketDetail.name}</h3>
                      <span className="market-card-sub">
                        {marketDetail.source_kind} · v{marketDetail.version ?? "?"}
                      </span>
                    </div>
                    <div className="market-market-detail-actions">
                      <button className="secondary-button" type="button"
                        disabled={marketRefreshBusy === marketDetail.marketplace_id}
                        onClick={() => void refreshMarket(marketDetail.marketplace_id)}>
                        {marketRefreshBusy === marketDetail.marketplace_id
                          ? t("catalog.marketRefreshing")
                          : t("catalog.marketRefresh")}
                      </button>
                      <button className="danger-button" type="button" disabled={marketBusy}
                        onClick={() => void removeMarket(marketDetail.marketplace_id)}>
                        {t("catalog.marketRemove")}
                      </button>
                    </div>
                  </div>
                  <div className="market-list">
                    {marketDetail.entries.map((entry) => {
                      const storeItem = (storeItems ?? []).find(
                        (item) => item.marketplace_id === marketDetail.marketplace_id && item.entry_name === entry.name,
                      );
                      const busy = storeInstallBusy === `${marketDetail.marketplace_id}/${entry.name}`;
                      const installed = storeItem?.installed ?? false;
                      return (
                        <div className="market-card market-entry-card" key={`${marketDetail.marketplace_id}/${entry.name}`}>
                          <div className="market-card-top">
                            <InitialBadge name={entry.name} />
                            <div className="market-card-main">
                              <span className="market-card-title">{entry.name}</span>
                              <span className="market-card-sub">
                                {entry.description ?? entry.source}
                              </span>
                            </div>
                            {installed ? (
                              <span className="market-tag is-status is-success">{t("catalog.storeInstalled")}</span>
                            ) : (
                              <button
                                className="primary-button market-entry-import"
                                type="button"
                                disabled={busy || !storeItem}
                                onClick={() => { if (storeItem) void runStoreInstall(storeItem); }}
                              >
                                {busy ? t("catalog.storeInstalling") : t("catalog.storeInstall")}
                              </button>
                            )}
                          </div>
                        </div>
                      );
                    })}
                  </div>
                </div>
              )}
            </div>
          </div>
        )}

        {tab === "imports" && (
          <div className="market-imports">
            <div className="market-import-form">
              <label htmlFor="import-kind">{t("catalog.importSource")}</label>
              <div className="market-import-row">
                <select
                  id="import-kind"
                  className="market-select"
                  value={importKind}
                  onChange={(event) => setImportKind(event.target.value as ImportSourceKind)}
                >
                  <option value="codebuddy-plugin">{t("catalog.importKindPlugin")}</option>
                  <option value="workbuddy-skill-market">{t("catalog.importKindSkills")}</option>
                  <option value="workbuddy-connector-market">{t("catalog.importKindConnectors")}</option>
                  <option value="workbuddy-cli-connector">{t("catalog.importKindCliConnector")}</option>
                </select>
                <input
                  className="market-input"
                  type="text"
                  placeholder={t("catalog.importPathPlaceholder")}
                  value={importPath}
                  onChange={(event) => setImportPath(event.target.value)}
                  onKeyDown={(event) => { if (event.key === "Enter") void runImport(); }}
                />
                <button className="primary-button" type="button" disabled={importBusy} onClick={() => void runImport()}>
                  {importBusy ? t("catalog.importing") : t("catalog.importRun")}
                </button>
              </div>
            </div>

            {importResult && (
              <button className="market-card market-import-result" type="button" onClick={() => void openImport(importResult.snapshot_id)}>
                <div className="market-card-top">
                  <InitialBadge name={importResult.name} />
                  <div className="market-card-main">
                    <span className="market-card-title">{importResult.name} <small>v{importResult.version}</small></span>
                    <span className="market-card-sub">
                      {importResult.source_kind} · {t("catalog.importComponents", { count: importResult.component_count })}
                    </span>
                  </div>
                  <span className={`status-dot ${importStatusClass(importResult.status)}`} aria-hidden="true" />
                </div>
                <Tags tags={[
                  importStatusLabel(t, importResult.status),
                  ...(importResult.reused ? [t("catalog.importReused")] : []),
                  ...(importResult.errors.length > 0 ? importResult.errors.slice(0, 1) : []),
                ]} />
              </button>
            )}

            <div className="market-list">
              {!client && <p className="market-empty">{t("catalog.connectFirst")}</p>}
              {client && imports === null && <p className="market-empty">{t("catalog.loadingImports")}</p>}
              {client && imports !== null && importsFiltered.length === 0 && (
                <p className="market-empty">{t("catalog.noImports")}</p>
              )}
              {importsFiltered.map((item) => (
                <button className="market-card" type="button" key={item.snapshot_id} onClick={() => void openImport(item.snapshot_id)}>
                  <div className="market-card-top">
                    <InitialBadge name={item.name} />
                    <div className="market-card-main">
                      <span className="market-card-title">{item.name} <small>v{item.version}</small></span>
                      <span className="market-card-sub">{item.source_kind} · {t("catalog.importComponents", { count: item.component_count })}</span>
                    </div>
                    <span className={`status-dot ${importStatusClass(item.status)}`} aria-hidden="true" />
                  </div>
                  <Tags tags={[importStatusLabel(t, item.status)]} />
                </button>
              ))}
            </div>
          </div>
        )}
      </div>

      {/* Detail drawer */}
      {drawer && (
        <div className="market-drawer-mask" onClick={closeDrawer} role="presentation">
          <aside className="market-drawer" role="dialog" aria-modal="true" onClick={(event) => event.stopPropagation()}>
            <button className="icon-button market-drawer-close" type="button" aria-label={t("common.close")} onClick={closeDrawer}>
              <X size={18} strokeWidth={1.7} />
            </button>
            {drawer === "skill" && skillDetail && <SkillDrawer detail={skillDetail} />}
            {drawer === "store" && storeDrawerItem && (
              <StoreDrawer
                item={storeDrawerItem}
                busy={storeInstallBusy === storeDrawerItem.id}
                result={storeInstallResult}
                onInstall={() => void runStoreInstall(storeDrawerItem)}
                rootUrl={client?.serverRootUrl}
              />
            )}
            {drawer === "connector" && connectorDetail && (
              <ConnectorDrawer
                detail={connectorDetail}
                authState={connectorDetailAuth}
                busy={detailBusy === connectorDetail.id}
                onProbe={() => void probeConnector(connectorDetail.id)}
                onAuthStart={() => void startAuth(connectorDetail.id)}
                onAuthRefresh={() => void refreshAuth(connectorDetail.id)}
                onLogout={() => void logoutConnector(connectorDetail.id)}
              />
            )}
            {drawer === "agent" && agentDetail && <AgentDrawer detail={agentDetail} />}
            {drawer === "team" && teamDetail && <TeamDrawer detail={teamDetail} />}
            {drawer === "import" && importDetail && (
              <ImportDrawer
                detail={importDetail}
                installStatus={installStatus}
                installResult={installResult}
                busy={detailBusy}
                onInstall={() => void runInstall(importDetail.snapshot_id)}
                onToggle={(kind, componentId) => void toggleInstall(kind, importDetail.snapshot_id, componentId)}
              />
            )}
            {detailBusy && !storeDrawerItem && !skillDetail && !connectorDetail && !agentDetail && !teamDetail && !importDetail && (
              <p className="market-empty">{t("catalog.loading")}</p>
            )}
          </aside>
        </div>
      )}
    </section>
  );
}

// ---------------------------------------------------------------------------
// grid + shared pieces
// ---------------------------------------------------------------------------

function importStatusLabel(t: (key: string, opts?: Record<string, unknown>) => string, status: string): string {
  const key = IMPORT_STATUS_KEYS[status];
  return key ? t(key) : t("catalog.stateOther", { status });
}

function MarketGrid<T>({
  loading,
  empty,
  emptyAction,
  items,
  renderItem,
}: {
  loading: boolean;
  empty: string;
  emptyAction?: React.ReactNode;
  items: T[];
  renderItem: (item: T) => React.ReactNode;
}) {
  const { t } = useTranslation();
  return (
    <div className="market-grid-wrap">
      {loading ? (
        <p className="market-empty">{t("catalog.loading")}</p>
      ) : items.length === 0 ? (
        <div className="market-empty-wrap">
          <p className="market-empty">{empty}</p>
          {emptyAction}
        </div>
      ) : (
        <div className="market-grid">
          {items.map((item) => renderItem(item))}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// drawers
// ---------------------------------------------------------------------------

function MetaRow({ label, value, mono }: { label: string; value?: string | null; mono?: boolean }) {
  if (value === undefined || value === null || value === "") return null;
  return (
    <div className="market-meta-row">
      <dt>{label}</dt>
      <dd className={mono ? "is-mono" : ""}>{value}</dd>
    </div>
  );
}

function MetaList({ label, values }: { label: string; values: string[] }) {
  if (values.length === 0) return null;
  return (
    <div className="market-meta-row">
      <dt>{label}</dt>
      <dd>{values.join(", ")}</dd>
    </div>
  );
}

function StoreDrawer({
  item,
  busy,
  result,
  onInstall,
  rootUrl,
}: {
  item: StoreItem;
  busy: boolean;
  result: StoreInstallResult | null;
  onInstall: () => void;
  rootUrl?: string;
}) {
  const { t } = useTranslation();
  const name = item.display_name?.zh || item.display_name?.en || item.name;
  const profession = item.profession?.zh || item.profession?.en;
  const description = item.display_description?.zh || item.display_description?.en || item.description;
  const tags = (item.tags ?? []).map((tag) => tag.zh ?? tag.en ?? "").filter(Boolean);
  const quickPrompts = item.quick_prompts ?? [];
  return (
    <div className="drawer-body">
      <div className="drawer-head">
        <StoreBadge item={item} size={44} rootUrl={rootUrl} />
        <div>
          <h2>{name}</h2>
          {profession && <div className="drawer-subtitle">{profession}</div>}
          <div className="drawer-chips">
            <span className="market-tag">{t("catalog.storeFromMarket", { market: item.marketplace_name })}</span>
            <span className="market-tag">{item.kind}</span>
            <span className="market-tag">v{item.version}</span>
          </div>
        </div>
      </div>
      {description && <p className="drawer-desc">{description}</p>}
      {quickPrompts.length > 0 && (
        <div className="drawer-quick-prompts">
          <h3>{t("catalog.agentQuickPrompts")}</h3>
          <ul>
            {quickPrompts.map((prompt, index) => (
              <li key={index}>{prompt.zh || prompt.en}</li>
            ))}
          </ul>
        </div>
      )}
      <dl className="market-meta">
        <MetaRow label={t("catalog.fieldVersion")} value={item.version} />
        <MetaRow label={t("catalog.fieldType")} value={item.kind} />
        <MetaRow label={t("catalog.fieldTransport")} value={`${item.source_kind} · ${item.marketplace_name}`} />
        <MetaList label={t("catalog.fieldSkills")} values={tags} />
      </dl>
      <div className="drawer-actions">
        {item.installed ? (
          <span className="market-tag is-status is-success">{t("catalog.storeInstalled")}</span>
        ) : (
          <button className="primary-button" type="button" disabled={busy} onClick={onInstall}>
            {busy ? t("catalog.storeInstalling") : t("catalog.storeInstall")}
          </button>
        )}
        {item.update_available && (
          <button className="secondary-button" type="button" disabled={busy} onClick={onInstall}>
            {busy ? t("catalog.storeUpdating") : t("catalog.storeUpdate")}
          </button>
        )}
        {result && (
          <span className="market-tag is-status is-success">
            {t("catalog.storeSnapInstalled", { count: result.installed_count })}
          </span>
        )}
      </div>
    </div>
  );
}

function SkillDrawer({ detail }: { detail: SkillDetail }) {
  const { t } = useTranslation();
  return (
    <div className="drawer-body">
      <div className="drawer-head">
        <InitialBadge name={detail.name} size={44} />
        <div>
          <h2>{detail.name}</h2>
          <div className="drawer-chips">
            <span className="market-tag">{detail.source}</span>
            <span className="market-tag">{detail.compatibility_status}</span>
          </div>
        </div>
      </div>
      {detail.description && <p className="drawer-desc">{detail.description}</p>}
      <dl className="market-meta">
        <MetaRow label={t("catalog.fieldMode")} value={detail.mode} />
        <MetaRow label={t("catalog.fieldInvocation")} value={detail.invocation_policy} />
        <MetaRow label={t("catalog.fieldVersion")} value={detail.version} />
        <MetaList label={t("catalog.fieldRequiredConnectors")} values={detail.required_connectors ?? []} />
      </dl>
      {detail.instructions_summary && (
        <details className="drawer-details" open>
          <summary>{t("catalog.fieldInstructions")}</summary>
          <pre>{detail.instructions_summary}</pre>
        </details>
      )}
    </div>
  );
}

function ConnectorDrawer({
  detail,
  authState,
  busy,
  onProbe,
  onAuthStart,
  onAuthRefresh,
  onLogout,
}: {
  detail: ConnectorDetail;
  authState?: string;
  busy: boolean;
  onProbe: () => void;
  onAuthStart: () => void;
  onAuthRefresh: () => void;
  onLogout: () => void;
}) {
  const { t } = useTranslation();
  const authenticated = authState === "authenticated";
  return (
    <div className="drawer-body">
      <div className="drawer-head">
        <InitialBadge name={detail.name} size={44} />
        <div>
          <h2>{detail.name}</h2>
          <div className="drawer-chips">
            <span className={`market-tag is-status ${connectorStateClass(detail.status)}`}>
              {stateLabel(t, CONNECTOR_STATE_KEYS, detail.status)}
            </span>
            {!detail.enabled && <span className="market-tag is-status is-warn">{t("catalog.disabled")}</span>}
          </div>
        </div>
      </div>
      <dl className="market-meta">
        <MetaRow label={t("catalog.fieldType")} value={detail.kind} />
        <MetaRow label={t("catalog.fieldTransport")} value={detail.transport_summary} mono />
        <MetaRow label={t("catalog.fieldAuth")} value={detail.auth_mode} />
        <MetaRow label={t("catalog.fieldNamespace")} value={detail.tool_filter ?? undefined} mono />
      </dl>
      <div className="drawer-actions">
        {detail.auth_mode === "oauth" ? (
          <>
            <span className={`market-tag is-status ${authenticated ? "is-success" : "is-warn"}`}>
              {stateLabel(t, AUTH_STATE_KEYS, authState ?? "not_authenticated")}
            </span>
            {!authenticated ? (
              <button className="quiet-button" type="button" onClick={onAuthStart} disabled={busy}>{t("catalog.authAuthorize")}</button>
            ) : (
              <button className="quiet-button" type="button" onClick={onLogout} disabled={busy}>{t("catalog.authRevoke")}</button>
            )}
            <button className="quiet-button" type="button" onClick={onAuthRefresh} disabled={busy}>{t("catalog.authRefreshStatus")}</button>
          </>
        ) : null}
        <button className="primary-button" type="button" onClick={onProbe} disabled={busy}>{t("catalog.authTest")}</button>
      </div>
      {busy && <p className="drawer-hint">{t("common.processing")}</p>}
      <details className="drawer-details" open={(detail.tools?.length ?? 0) > 0}>
        <summary>{t("catalog.tools", { count: detail.tools?.length ?? 0 })}</summary>
        {(detail.tools ?? []).length === 0 ? (
          <p className="drawer-hint">{t("catalog.noToolsHint")}</p>
        ) : (
          <ul className="drawer-tools">
            {(detail.tools ?? []).map((tool) => (
              <li key={tool.name}>
                <code>{tool.name}</code>
                {tool.description && <span>{tool.description}</span>}
              </li>
            ))}
          </ul>
        )}
      </details>
    </div>
  );
}

function AgentDrawer({ detail }: { detail: AgentDetail }) {
  const { t } = useTranslation();
  const displayName = detail.display_name?.zh || detail.display_name?.en || detail.name;
  const profession = detail.profession?.zh || detail.profession?.en;
  return (
    <div className="drawer-body">
      <div className="drawer-head">
        {detail.avatar_url ? (
          <img className="drawer-avatar" src={detail.avatar_url} alt={displayName} loading="lazy" />
        ) : (
          <InitialBadge name={displayName} size={44} />
        )}
        <div>
          <h2>{displayName}</h2>
          {profession && <div className="drawer-subtitle">{profession}</div>}
          <div className="drawer-chips">
            <span className="market-tag">{detail.source}</span>
            <span className="market-tag">{detail.compatibility_status}</span>
          </div>
        </div>
      </div>
      {(detail.display_description?.zh || detail.display_description?.en || detail.description) && (
        <p className="drawer-desc">{detail.display_description?.zh || detail.display_description?.en || detail.description}</p>
      )}
      {detail.quick_prompts && detail.quick_prompts.length > 0 && (
        <div className="drawer-quick-prompts">
          <h3>{t("catalog.agentQuickPrompts")}</h3>
          <ul>
            {detail.quick_prompts.map((prompt, index) => (
              <li key={index}>{prompt.zh || prompt.en}</li>
            ))}
          </ul>
        </div>
      )}
      <dl className="market-meta">
        <MetaRow label={t("catalog.fieldVersion")} value={detail.version} />
        <MetaRow label={t("catalog.fieldModel")} value={detail.model_summary} />
        <MetaRow label={t("catalog.fieldEffort")} value={detail.effort} />
        <MetaRow label={t("catalog.fieldMaxTurns")} value={detail.max_turns?.toString()} />
        <MetaRow label={t("catalog.fieldToolPolicy")} value={detail.tool_policy_summary} />
        <MetaList label={t("catalog.fieldDisallowedTools")} values={detail.disallowed_tools ?? []} />
        <MetaList label={t("catalog.fieldSkills")} values={detail.skills ?? []} />
        <MetaRow label={t("catalog.fieldMemory")} value={detail.memory} />
        <MetaRow label={t("catalog.fieldBackground")} value={detail.background} />
        <MetaRow label={t("catalog.fieldIsolation")} value={detail.isolation} />
      </dl>
      {detail.permission_mode_ignored && (
        <p className="drawer-hint">{t("catalog.agentPermissionIgnored")}</p>
      )}
    </div>
  );
}

function TeamDrawer({ detail }: { detail: TeamDetail }) {
  const { t } = useTranslation();
  return (
    <div className="drawer-body">
      <div className="drawer-head">
        <InitialBadge name={detail.name} size={44} />
        <div>
          <h2>{detail.name}</h2>
          <div className="drawer-chips">
            <span className="market-tag">{detail.source}</span>
            <span className="market-tag">{detail.compatibility_status}</span>
          </div>
        </div>
      </div>
      {detail.description && <p className="drawer-desc">{detail.description}</p>}
      <dl className="market-meta">
        <MetaRow label={t("catalog.fieldVersion")} value={detail.version} />
        <MetaRow label={t("catalog.fieldLead")} value={detail.lead_agent_id} mono />
        <MetaList label={t("catalog.fieldMembers")} values={detail.member_agent_ids ?? []} />
        <MetaRow label={t("catalog.fieldPlanner")} value={detail.planner_policy} />
        <MetaRow label={t("catalog.fieldWorkflowLimits")} value={JSON.stringify(detail.workflow_limits)} mono />
        <MetaList label={t("catalog.fieldCapabilities")} values={detail.team_runtime_capabilities ?? []} />
      </dl>
    </div>
  );
}

type InstallToggleKind = "enable" | "disable" | "uninstall";

function installStateLabel(t: ReturnType<typeof useTranslation>["t"], state: InstallState): string {
  switch (state) {
    case "installed": return t("catalog.installStateInstalled");
    case "disabled": return t("catalog.installStateDisabled");
    default: return t("catalog.installStateNotInstalled");
  }
}

function ImportDrawer({
  detail,
  installStatus,
  installResult,
  busy,
  onInstall,
  onToggle,
}: {
  detail: ImportDetail;
  installStatus: InstallStatus | null;
  installResult: InstallResult | null;
  busy: string | null;
  onInstall: () => void;
  onToggle: (kind: InstallToggleKind, componentId: string) => void;
}) {
  const { t } = useTranslation();
  const statusText = importStatusLabel(t, detail.status);
  const stateByComponent = new Map<string, InstallState>();
  installStatus?.components.forEach((component) => stateByComponent.set(component.id, component.state));
  const anyInstalled = installStatus?.components.some((component) => component.state !== "not-installed") ?? false;

  return (
    <div className="drawer-body">
      <div className="drawer-head">
        <InitialBadge name={detail.name} size={44} />
        <div>
          <h2>{detail.name} <small>v{detail.version}</small></h2>
          <div className="drawer-chips">
            <span className={`market-tag is-status ${importStatusClass(detail.status)}`}>{statusText}</span>
            <span className="market-tag">{detail.source_kind}</span>
          </div>
        </div>
      </div>
      <div className="drawer-actions">
        {!anyInstalled && (
          <button
            className="primary-button"
            type="button"
            disabled={busy !== null}
            onClick={onInstall}
          >
            {busy === detail.snapshot_id
              ? t("catalog.installing")
              : t("catalog.installRun")}
          </button>
        )}
        {installResult && (
          <span className="market-tag is-status is-success">
            {t("catalog.installDone", { count: installResult.installed_count })}
          </span>
        )}
        {installResult && installResult.skipped.length > 0 && (
          <span className="market-tag is-status is-warn">
            {t("catalog.installSkipped", { count: installResult.skipped.length })}
          </span>
        )}
      </div>
      <dl className="market-meta">
        <MetaRow label={t("catalog.importDigest")} value={detail.content_digest} mono />
        <MetaRow label={t("catalog.importComponents", { count: detail.components.length })} value={String(detail.components.length)} />
      </dl>
      <CompatChips triple={detail.component_status} />
      {detail.warnings.length > 0 && (
        <details className="drawer-details" open>
          <summary>{t("catalog.importWarnings", { count: detail.warnings.length })}</summary>
          <ul className="drawer-notes">{detail.warnings.map((warning, index) => <li key={index}>{warning}</li>)}</ul>
        </details>
      )}
      {detail.errors.length > 0 && (
        <details className="drawer-details" open>
          <summary>{t("catalog.importErrors", { count: detail.errors.length })}</summary>
          <ul className="drawer-notes">{detail.errors.map((error, index) => <li key={index}>{error}</li>)}</ul>
        </details>
      )}
      <details className="drawer-details" open>
        <summary>{t("catalog.importComponents", { count: detail.components.length })}</summary>
        <ul className="drawer-tools">
          {detail.components.map((component) => {
            const state = stateByComponent.get(component.id) ?? "not-installed";
            return (
              <li key={component.id}>
                <div className="drawer-tool-row">
                  <code>{component.kind}: {component.name}</code>
                  <span className={`market-tag is-status ${installStateClass(state)}`}>
                    {installStateLabel(t, state)}
                  </span>
                  <div className="drawer-tool-actions">
                    {state === "installed" && (
                      <button className="quiet-button" type="button" disabled={busy !== null}
                        onClick={() => onToggle("disable", component.id)}>
                        {t("catalog.installDisable")}
                      </button>
                    )}
                    {state === "disabled" && (
                      <button className="quiet-button" type="button" disabled={busy !== null}
                        onClick={() => onToggle("enable", component.id)}>
                        {t("catalog.installEnable")}
                      </button>
                    )}
                    {state !== "not-installed" && (
                      <button className="quiet-button" type="button" disabled={busy !== null}
                        onClick={() => onToggle("uninstall", component.id)}>
                        {t("catalog.installUninstall")}
                      </button>
                    )}
                  </div>
                </div>
                <CompatChips triple={component.compatibility} />
              </li>
            );
          })}
        </ul>
      </details>
    </div>
  );
}

function installStateClass(state: InstallState): string {
  switch (state) {
    case "installed": return "is-success";
    case "disabled": return "is-warn";
    default: return "";
  }
}
