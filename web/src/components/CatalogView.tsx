/**
 * Agent Store catalog view — three noun tabs (专家 / 技能 / 连接器), each with a
 * *browse* surface (the store projection) and an *installed* surface that the
 * tab's right-hand button switches to.
 *
 * A pure protocol consumer over the App Server link (`src/lib/client.ts`).
 * Cards are a circular initial badge (or the entry's own avatar), two-line
 * clamps, at most two metadata tags, plus a right-side detail drawer. Every
 * value is real protocol data; nothing is fabricated by the client.
 *
 * Two surfaces this page used to own live elsewhere now, because they are
 * configuration rather than browsing: the marketplace registry is the settings
 * dialog's 市场源 section (`catalog/MarketSourcesPanel`), and the import form
 * with its history opens as a dialog (`catalog/ImportPanel`) from the 技能 /
 * 连接器 tabs' buttons.
 *
 * Capabilities are server-driven: a noun tab only appears when `initialize`
 * advertised it.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Bot,
  Check,
  ChevronRight,
  CircleAlert,
  LoaderCircle,
  Menu,
  Plug,
  Plus,
  RefreshCw,
  Search,
  Server,
  Sparkles,
  Upload,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { formatError, isRetryableError } from "../lib/errors";
import { hasAnyPublishedAt, sortNewestFirst } from "../lib/store-sort";
import {
  pickLocalized,
  pickLocalizedList,
  useLocalizedLang,
} from "../ui/localize";
import { useAppStore } from "../store/appStore";
import {
  SkillWriteActionsView,
  SkillWriteDialogs,
  SkillWriteToolbarView,
  originLabelI18nKey,
  type SkillWriteMode,
} from "./skills/SkillWriteSurface";
import type {
  AgentDetail,
  AgentSummary,
  ConnectorDetail,
  ConnectorSummary,
  ImportSourceKind,
  SkillDetail,
  SkillSummary,
  StoreInstallResult,
  StoreItem,
  StoreItemKind,
  TeamDetail,
  TeamSummary,
} from "../lib/protocol";
import { IconButton } from "./IconButton";
import { DialogShell } from "./dialogs/DialogShell";
import { McpManagerDialog } from "./dialogs/McpSettingsSection";
import { ImportPanel } from "./catalog/ImportPanel";
import {
  AUTH_STATE_KEYS,
  AgentBadge,
  AvatarBadge,
  CONNECTOR_STATE_KEYS,
  CompatChips,
  InitialBadge,
  MetaList,
  MetaRow,
  SEMANTIC_KEYS,
  StoreBadge,
  Tags,
  agentDisplayName,
  connectorStateClass,
  importStatusLabel,
  installStateClass,
  installStateLabel,
  marketKindLabel,
  semanticLabel,
  stateLabel,
  storeKindLabel,
  type InstallToggleKind,
} from "./catalog/shared";

/**
 * Top-level noun tabs: one per component family. Deliberately *nouns*, not the
 * verbs the page used to expose (`store` / `sources` / `installed` / `imports`)
 * — the marketplace registry now lives in the settings dialog and the import
 * surface opens from the contextual buttons, so the page itself is about the
 * three families of thing.
 */
type CatalogNoun = "experts" | "skills" | "connectors";
/** The 专家 tab's own split (`catalog.kindAgent` / `catalog.kindTeam`). */
type ExpertKind = "experts" | "teams";
/** Browse the store catalog vs. the installed inventory of the same noun. */
type CatalogSurface = "browse" | "installed";
type PanelKind = "store" | "skill" | "connector" | "agent" | "team";

// ---------------------------------------------------------------------------
// semantic helpers
// ---------------------------------------------------------------------------

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
  // `02` §11.1: an entry the server already refuses (e.g. `strict=true` with no
  // `plugin.json` of its own) stays listed so the reason is readable, but the
  // install action must not be offered — it would be refused anyway.
  if (item.blocked_reason) {
    return (
      <span
        className="market-store-float is-blocked"
        role="note"
        title={item.blocked_reason}
        aria-label={`${t("catalog.storeBlocked")}: ${item.blocked_reason}`}
      >
        <CircleAlert size={14} strokeWidth={1.7} />
      </span>
    );
  }
  if (item.installed) {
    // A note, never a control. Clicking an installed item only ever re-ran
    // `store/install-entry`, which is a documented no-op for an installed entry
    // — and for a pending update it was worse: re-importing would have been a
    // hidden upgrade. `16` §6: no shell control that does nothing when pressed.
    const pendingUpdate = item.update_available;
    return (
      <span
        className="market-store-float is-installed"
        role="note"
        title={pendingUpdate ? t("catalog.storeUpdate") : t("catalog.storeInstalled")}
        aria-label={pendingUpdate ? t("catalog.storeUpdate") : t("catalog.storeInstalled")}
      >
        {pendingUpdate ? <RefreshCw size={14} strokeWidth={1.7} /> : <Check size={14} strokeWidth={2} />}
      </span>
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

// ---------------------------------------------------------------------------
// Catalog view
// ---------------------------------------------------------------------------

export function CatalogView() {
  const { t } = useTranslation();
  const lang = useLocalizedLang();
  const client = useAppStore((s) => s.client);
  const capabilities = useAppStore((s) => s.client?.initializeInfo?.capabilities ?? null);
  const setSidebarOpen = useAppStore((s) => s.setSidebarOpen);
  const pushToast = useAppStore((s) => s.pushToast);
  const openSettings = useAppStore((s) => s.openSettings);

  const [nounState, setNoun] = useState<CatalogNoun>("experts");
  const [surface, setSurface] = useState<CatalogSurface>("browse");
  const [expertKind, setExpertKind] = useState<ExpertKind>("experts");
  const [query, setQuery] = useState("");
  /** Which import dialog is open, and with which source kind preselected. */
  const [importFor, setImportFor] = useState<ImportSourceKind | null>(null);
  /** The MCP declarations dialog (连接器 tab): the same manager the settings
   *  dialog hosts, opened where the operator is already looking at servers. */
  const [mcpOpen, setMcpOpen] = useState(false);

  /**
   * Noun tabs the host actually advertised — the same `!== false` convention the
   * installed chips used (a missing flag means "not said", not "no").
   */
  const visibleNouns = useMemo<CatalogNoun[]>(() => {
    const nouns: CatalogNoun[] = [];
    if (capabilities?.agents !== false || capabilities?.teams !== false) nouns.push("experts");
    if (capabilities?.skills !== false) nouns.push("skills");
    if (capabilities?.connectors !== false) nouns.push("connectors");
    return nouns;
  }, [capabilities]);

  /** The selection, clamped to the advertised set: the tab that owns the body
   *  is always one of the tabs on screen. */
  const noun: CatalogNoun = visibleNouns.includes(nounState) ? nounState : (visibleNouns[0] ?? "experts");

  // store (winget-style aggregated catalog)
  const [storeItems, setStoreItems] = useState<StoreItem[] | null>(null);
  const [sortBy, setSortBy] = useState<"default" | "name" | "newest">("default");
  const [storeInstallBusy, setStoreInstallBusy] = useState<string | null>(null);
  /** True while the builtin marketplaces are still mirroring in the background
   *  (D-SDK-1 ①): the catalog is legitimately incomplete, not empty. */
  const [storePending, setStorePending] = useState(false);

  // lists (null = loading)
  const [skills, setSkills] = useState<SkillSummary[] | null>(null);
  const [connectors, setConnectors] = useState<ConnectorSummary[] | null>(null);
  const [agents, setAgents] = useState<AgentSummary[] | null>(null);
  const [teams, setTeams] = useState<TeamSummary[] | null>(null);

  // detail drawer
  const [drawer, setDrawer] = useState<PanelKind | null>(null);
  const [skillDetail, setSkillDetail] = useState<SkillDetail | null>(null);
  /** Which skill write dialog is open (W12 / `16` R17); `null` = none. */
  const [skillWrite, setSkillWrite] = useState<SkillWriteMode>(null);
  const [connectorDetail, setConnectorDetail] = useState<ConnectorDetail | null>(null);
  const [agentDetail, setAgentDetail] = useState<AgentDetail | null>(null);
  const [teamDetail, setTeamDetail] = useState<TeamDetail | null>(null);
  const [detailBusy, setDetailBusy] = useState<string | null>(null);

  // store drawer (winget-style item detail + install)
  const [storeDrawerItem, setStoreDrawerItem] = useState<StoreItem | null>(null);
  const [storeInstallResult, setStoreInstallResult] = useState<StoreInstallResult | null>(null);

  // connector auth state map (id -> oauth state)
  const [authMap, setAuthMap] = useState<Map<string, string>>(new Map());
  const [error, setError] = useState<string | null>(null);

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
    setError(null);
    setReloadTick((tick) => tick + 1);
  }, []);

  // One effect loads the catalog slices it renders; a single slice failing must
  // never blank the rest. The import history and the marketplace registry have
  // their own readers (`ImportPanel` / `MarketSourcesPanel`), so they are not
  // fetched here any more.
  useEffect(() => {
    if (!client) return;
    let cancelled = false;
    const load = async () => {
      try {
        const [storeList, skillList, connectorList, agentList, teamList] = await Promise.all([
          capabilities?.store ? client.listStore().catch(() => null) : Promise.resolve(null),
          capabilities?.skills ? client.skills.list().catch(() => []) : Promise.resolve([]),
          capabilities?.connectors ? client.connectors.list().catch(() => []) : Promise.resolve([]),
          capabilities?.agents ? client.agents.list().catch(() => []) : Promise.resolve([]),
          capabilities?.teams ? client.teams.list().catch(() => []) : Promise.resolve([]),
        ]);
        if (cancelled || !activeRef.current) return;
        setStoreItems(storeList?.items ?? null);
        setStorePending(storeList?.markets_pending ?? false);
        setSkills(skillList);
        setConnectors(connectorList);
        setAgents(agentList);
        setTeams(teamList);
      } catch (caught) {
        if (cancelled || !activeRef.current) return;
        reportError(caught);
        setStoreItems(null);
        setSkills(null);
        setConnectors(null);
        setAgents(null);
        setTeams(null);
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, [client, capabilities, reloadTick]);

  // D-SDK-1 ①: a page loaded during the background marketplace warm-up sees an
  // incomplete catalog. `store/list` reports `markets_pending`, so re-list a few
  // times instead of making the user hit refresh. Bounded: 5 × 20s, then stop.
  const pendingRetriesRef = useRef(0);
  useEffect(() => {
    if (!storePending || !client) {
      pendingRetriesRef.current = 0;
      return;
    }
    if (pendingRetriesRef.current >= 5) return;
    pendingRetriesRef.current += 1;
    const timer = window.setTimeout(reload, 20_000);
    return () => window.clearTimeout(timer);
  }, [storePending, client, reloadTick, reload]);

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

  /**
   * Flip one connector's host-level `enabled` (doc `28` §4.5).
   *
   * Same first-party route as the composer's switch. The switch's own value comes
   * from the **response** (the route is a toggle, so the value we asked for is
   * not an answer); the host-derived `status` is then re-read from
   * `connector/list` so the drawer's status chip and the card's status dot agree
   * with the new state instead of keeping the pre-toggle projection.
   */
  const toggleConnectorEnabled = useCallback(async (connectorId: string) => {
    if (!client) return;
    setDetailBusy(connectorId);
    try {
      const result = await client.toggleMcpServerEnabled(connectorId);
      if (!activeRef.current) return;
      setConnectors((current) =>
        current?.map((connector) =>
          connector.id === connectorId ? { ...connector, enabled: result.enabled } : connector,
        ) ?? current,
      );
      setConnectorDetail((current) =>
        current && current.id === connectorId ? { ...current, enabled: result.enabled } : current,
      );
      const fresh = await client.connectors.list();
      if (!activeRef.current) return;
      setConnectors(fresh);
      const updated = fresh.find((connector) => connector.id === connectorId);
      if (updated) {
        setConnectorDetail((current) =>
          current && current.id === connectorId
            ? { ...current, enabled: updated.enabled, status: updated.status }
            : current,
        );
      }
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
      // Refresh the aggregated store so the installed state flips. `reload()`
      // also re-lists the installed surfaces (`agent/list` / `skill/list` /
      // `connector/list`), which is where 「我的专家」 reads from — refreshing
      // only `store/list` here left the installed lists stale, so an entry the
      // card just marked installed stayed invisible on the installed tab until
      // a manual page reload. (The dedicated store-refresh line below is kept:
      // it is what keeps the open drawer's item in sync without a full spin.)
      reload();
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
      // W8 余项: the install used to end silently unless the drawer stayed open.
      pushToast(
        "success",
        result.reused ? "catalog.storeInstallReused" : "catalog.storeInstallDone",
        { name: item.name },
      );
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
    } finally {
      if (activeRef.current) setStoreInstallBusy(null);
    }
  }, [client, pushToast, reload]);

  const closeDrawer = useCallback(() => {
    setDrawer(null);
    setStoreDrawerItem(null);
    setStoreInstallResult(null);
    setSkillDetail(null);
    setConnectorDetail(null);
    setAgentDetail(null);
    setTeamDetail(null);
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

  const visibleConnectors = useMemo(() => {
    return (connectors ?? []).filter((connector) => {
      const text = `${connector.name} ${connector.description ?? ""} ${connector.kind} ${connector.auth_mode}`.toLowerCase();
      return text.includes(query.toLowerCase());
    });
  }, [connectors, query]);

  const visibleAgents = useMemo(() => {
    return (agents ?? []).filter((agent) => {
      const text = `${agent.name} ${agent.description ?? ""} ${agent.source}`.toLowerCase();
      return text.includes(query.toLowerCase());
    });
  }, [agents, query]);

  const visibleTeams = useMemo(() => {
    return (teams ?? []).filter((team) => {
      const text = `${team.name} ${team.description ?? ""} ${team.source}`.toLowerCase();
      return text.includes(query.toLowerCase());
    });
  }, [teams, query]);

  /** The store kind the current noun tab browses (the 专家 tab splits in two). */
  const storeKindForNoun: StoreItemKind =
    noun === "experts"
      ? (expertKind === "experts" ? "agent" : "team")
      : noun === "skills"
        ? "skill"
        : "connector";

  // Store items: the current noun's kind + search across display metadata.
  const visibleStoreItems = useMemo(() => {
    const byKind = (storeItems ?? []).filter((item) => item.kind === storeKindForNoun);
    const needle = query.trim().toLowerCase();
    // The search index deliberately keeps *both* languages (plus the
    // baseline fields): a Chinese reader may still type an English term from
    // the manifest. Only the rendered label is language-resolved (D8=A).
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
        (pickLocalized(a.display_name, lang) || a.name).localeCompare(
          pickLocalized(b.display_name, lang) || b.name,
          lang,
        ),
      );
    }
    if (sortBy === "newest") {
      // Ordering rules live in `lib/store-sort` so they can be unit-tested
      // without a connected client; the comparison is deliberately not inlined.
      return sortNewestFirst(filtered, (item) => pickLocalized(item.display_name, lang) || item.name, lang);
    }
    return filtered;
    // `lang` is a dependency because both name comparisons use the *rendered*
    // label, which is language-resolved.
  }, [storeItems, storeKindForNoun, query, sortBy, lang]);

  /**
   * Whether this noun's catalog carries any date at all.
   *
   * The 最新 sort is offered only then: a sort control over a field no entry
   * has is a control that does nothing, and today's real markets declare no
   * `publishedAt` — so without this the option would be a visibly dead button.
   * Computed over the kind (not the search results), so typing in the search
   * box cannot make the control appear and disappear.
   */
  const hasPublishedDates = useMemo(
    () => hasAnyPublishedAt((storeItems ?? []).filter((item) => item.kind === storeKindForNoun)),
    [storeItems, storeKindForNoun],
  );

  /** Store teams, used as the 精选场景 row of the 专家 tab's browse surface. */
  const visibleStoreTeams = useMemo(
    () => (storeItems ?? []).filter((item) => item.kind === "team"),
    [storeItems],
  );

  const connectorDetailAuth = connectorDetail ? authMap.get(connectorDetail.id) : undefined;

  /**
   * The search box's placeholder for whatever the current tab+surface shows.
   * (The tab version asked `searchPlaceholder[tab]` against a `Record` that
   * only knew about tabs: the `installed` entry always said "search experts"
   * whatever kind was on screen, and `sources` claimed an imports search box
   * that tab never rendered.)
   */
  const searchPlaceholder =
    noun === "skills"
      ? t("catalog.searchSkills")
      : noun === "connectors"
        ? t("catalog.searchConnectors")
        : expertKind === "teams"
          ? t("catalog.searchTeams")
          : t("catalog.searchAgents");

  /** Installed count for the tab's right-hand button, when it has one. The 专家
   *  tab counts whichever of its two kinds is on screen. */
  const installedCount =
    noun === "skills"
      ? skills?.length
      : noun === "connectors"
        ? connectors?.length
        : expertKind === "experts"
          ? agents?.length
          : teams?.length;

  return (
    <section className="market-view" aria-label={t("catalog.ariaLabel")}>
      {/* The page's whole toolbar, in one row. It replaced three stacked bands:
          a 58px title header, a 54px tab row, and a second 46px row that carried
          nothing but the sort on the tabs with no split of their own. The header
          went first because its text repeated the sidebar's own
          「专家·技能·连接器」 entry (which toggles back to chat) *and* the three
          tabs below it; with it gone the back control had nothing left to do
          either, so reload moved to the right end of this row.

          The noun tabs are a segmented control rather than three more bordered
          buttons: the same row also carries this noun's actions, and when both
          wore the same pill the row read as five equal choices.

          The tabs are one per component family. The page used to expose four
          verbs (`应用商店 / 市场源 / 导入 / 已安装`); the marketplace registry
          moved to the settings dialog's 市场源 section and the import surface
          opens from the tab's own right-hand buttons, so what remains is what
          the page is about.

          Reachability note, kept because it is the invariant that keeps getting
          broken: every surface needs a control that reaches it on a *non-empty*
          store, not only a button in some empty state. That is why the actions
          below are rendered unconditionally rather than only when the
          corresponding list happens to be empty. */}
      <div className="market-toolbar market-toolbar-store">
        {/* ≤680px the sidebar slides off-canvas, so this page needs its own way
            back to it — the same `.mobile-menu-button` the chat topbar shows. */}
        <IconButton label={t("topbar.openNav")} className="mobile-menu-button" onClick={() => setSidebarOpen(true)}>
          <Menu size={19} strokeWidth={1.7} />
        </IconButton>

        <div className="market-seg" role="tablist" aria-label={t("catalog.nounTabsLabel")}>
          {([
            { key: "experts" as const, icon: <Bot size={15} strokeWidth={1.7} />, label: t("catalog.tabExperts") },
            { key: "skills" as const, icon: <Sparkles size={15} strokeWidth={1.7} />, label: t("catalog.tabSkills") },
            { key: "connectors" as const, icon: <Plug size={15} strokeWidth={1.7} />, label: t("catalog.tabConnectors") },
          ] as { key: CatalogNoun; icon: React.ReactNode; label: string }[])
            .filter((entry) => visibleNouns.includes(entry.key))
            .map((entry) => (
            <button
              key={entry.key}
              className={`market-seg-item ${noun === entry.key ? "is-active" : ""}`}
              type="button"
              role="tab"
              aria-selected={noun === entry.key}
              onClick={() => { setNoun(entry.key); setSurface("browse"); setQuery(""); }}
            >
              {entry.icon}
              <span>{entry.label}</span>
            </button>
          ))}
        </div>

        {/* Sub-toolbar: the 专家 tab's own split. Only 专家 has a second real
            axis (专家 vs 专家团 are two wire kinds); 技能 and 连接器 have none.
            The reference's 「推荐 | SkillHub | 套件」 triple is deliberately not
            reproduced: 推荐 needs a curation flag we do not have, 套件 has no
            entity at all, and SkillHub is one specific upstream marketplace — a
            segmented control over no data is a control that lies. */}
        {noun === "experts" && (
          <div className="market-cats" role="group" aria-label={t("catalog.expertKindLabel")}>
            <button
              className={`market-chip ${expertKind === "experts" ? "is-active" : ""}`}
              type="button"
              aria-pressed={expertKind === "experts"}
              onClick={() => { setExpertKind("experts"); setQuery(""); }}
            >
              {t("catalog.kindAgent")}
            </button>
            <button
              className={`market-chip ${expertKind === "teams" ? "is-active" : ""}`}
              type="button"
              aria-pressed={expertKind === "teams"}
              onClick={() => { setExpertKind("teams"); setQuery(""); }}
            >
              {t("catalog.kindTeam")}
            </button>
          </div>
        )}

        <label className="market-search market-search-inline">
          <Search size={15} strokeWidth={1.7} />
          <input
            type="search"
            value={query}
            placeholder={searchPlaceholder}
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>

        {/* Sorting only applies to the store projection (the installed lists
            have no ordering of their own on the wire). */}
        {surface === "browse" && (
          <div className="market-sort" role="group" aria-label={t("catalog.sortLabel")}>
            <button
              className={`market-chip market-sort-chip ${sortBy === "default" ? "is-active" : ""}`}
              type="button"
              onClick={() => setSortBy("default")}
            >
              {t("catalog.sortDefault")}
            </button>
            {/* Only when the data can answer it — see `hasPublishedDates`. */}
            {hasPublishedDates && (
              <button
                className={`market-chip market-sort-chip ${sortBy === "newest" ? "is-active" : ""}`}
                type="button"
                onClick={() => setSortBy("newest")}
              >
                {t("catalog.sortNewest")}
              </button>
            )}
            <button
              className={`market-chip market-sort-chip ${sortBy === "name" ? "is-active" : ""}`}
              type="button"
              onClick={() => setSortBy("name")}
            >
              {t("catalog.sortName")}
            </button>
          </div>
        )}

        {/* Contextual actions: this noun's inventory, plus the way to add
            something to it. They belong to the page because they change *what
            it lists*; a settings dialog would make them a two-step detour. */}
        <div className="market-actions">
          <button
            className={`market-action ${surface === "installed" ? "is-active" : ""}`}
            type="button"
            aria-pressed={surface === "installed"}
            onClick={() => { setSurface(surface === "installed" ? "browse" : "installed"); setQuery(""); }}
          >
            {noun === "experts"
              ? t("catalog.myExperts")
              : t("catalog.myInstalled", { count: installedCount ?? 0 })}
          </button>
          {noun === "skills" && (
            <button className="market-action" type="button" onClick={() => setImportFor("workbuddy-skill-market")}>
              <Upload size={15} strokeWidth={1.7} />
              <span>{t("catalog.addSkill")}</span>
            </button>
          )}
          {noun === "connectors" && (
            <>
              <button className="market-action" type="button" onClick={() => setMcpOpen(true)}>
                <Server size={15} strokeWidth={1.7} />
                <span>{t("catalog.mcpManage")}</span>
              </button>
              <button className="market-action" type="button" onClick={() => setImportFor("workbuddy-connector-market")}>
                <Plus size={15} strokeWidth={1.7} />
                <span>{t("catalog.customConnector")}</span>
              </button>
            </>
          )}
        </div>

        <button className="icon-button" type="button" aria-label={t("catalog.reload")} title={t("catalog.reloadTitle")} onClick={reload}>
          <RefreshCw size={16} strokeWidth={1.7} />
        </button>
      </div>

      {error && (
        <div className="market-alert" role="alert">
          <CircleAlert size={15} strokeWidth={1.8} />
          <span>{error}</span>
          <button type="button" aria-label={t("common.closeError")} onClick={() => setError(null)}>✕</button>
        </div>
      )}

      {/* Body */}
      <div className="market-body">
        {surface === "browse" && (
          <>
            {/* 精选场景 — the reference's top row. It reuses the committed
                pattern (teams rendered as scene cards) but sources it from the
                *store* projection, so the row exists before anything is
                installed.

                Deviation from the reference, stated rather than faked: a store
                team carries no member list (`members` rides in the market
                manifest and is dropped by the projection) and no cover image, so
                the card shows its own description on the existing gradient —
                not a photo, and not a member collage we cannot fill. The
                marketplace name is also dropped here: it read as a stray
                "experts" badge on every card, and which market a card came
                from is already the 传输 row in its drawer. */}
            {noun === "experts" && expertKind === "experts" && visibleStoreTeams.length > 0 && (
              <>
                <h2 className="market-section">{t("catalog.featuredScenes")}</h2>
                <div className="market-scenes" role="list">
                  {visibleStoreTeams.map((item) => (
                    <button className="market-scene" type="button" role="listitem" key={item.id} onClick={() => openStoreItem(item)}>
                      <div className="market-scene-head">
                        <span className="market-scene-title">{pickLocalized(item.display_name, lang) || item.name}</span>
                      </div>
                      <div className="market-scene-body">
                        {pickLocalized(item.display_description, lang) || item.description || t("catalog.noDescription")}
                      </div>
                    </button>
                  ))}
                </div>
              </>
            )}
            {noun === "experts" && expertKind === "experts" && (
              <h2 className="market-section">{t("catalog.experts")}</h2>
            )}
            <MarketGrid
              loading={!client || storeItems === null}
              empty={storePending ? t("catalog.storePending") : t("catalog.storeEmpty")}
              emptyAction={(
                <button className="market-empty-action" type="button" onClick={() => openSettings("market")}>
                  {t("catalog.emptyGoSources")}
                </button>
              )}
              items={visibleStoreItems}
            renderItem={(item) => (
              <div className="market-card market-store-card" key={item.id}>
                <div
                  className="market-store-main"
                  onClick={() => openStoreItem(item)}
                >
                  <div className="market-card-top">
                    <StoreBadge item={item} rootUrl={client?.serverRootUrl} />
                    <div className="market-card-main">
                      <span className="market-card-title">{pickLocalized(item.display_name, lang) || item.name}</span>
                      {pickLocalized(item.profession, lang) ? (
                        <span className="market-card-author">{pickLocalized(item.profession, lang)}</span>
                      ) : null}
                    </div>
                  </div>
                  {(pickLocalized(item.display_description, lang) || item.description) && (
                    <span className="market-card-desc">{pickLocalized(item.display_description, lang) || item.description}</span>
                  )}
                </div>
                <div className="market-card-foot">
                  <div className="market-tags">
                    {/* `02` §11.1: stated on the card, not only in the drawer —
                        the reason is why the install button is gone. */}
                    {item.blocked_reason ? (
                      <span className="market-tag is-blocked" title={item.blocked_reason}>
                        {t("catalog.storeBlocked")}
                      </span>
                    ) : null}
                    {pickLocalizedList(item.tags, lang).slice(0, 3).map((text) => {
                      return text ? <span className="market-tag" key={text}>{text}</span> : null;
                    })}
                    {/* The market's own publication date, shown only when it
                        declared one — never a placeholder, and never derived
                        from the import time. It is muted because it explains
                        the 最新 sort rather than describing the entry. */}
                    {item.published_at ? (
                      <span className="market-tag is-muted">{item.published_at}</span>
                    ) : null}
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
          </>
        )}

        {surface === "installed" && noun === "skills" && (
          <>
            <SkillWriteToolbarView onCreated={() => setSkillWrite({ kind: "create" })} />
            <MarketGrid
              loading={!client || skills === null}
              empty={t("catalog.noSkills")}
              emptyAction={
                <button className="market-empty-action" type="button" onClick={() => setSurface("browse")}>
                  {t("catalog.emptyGoStore")}
                </button>
              }
              items={visibleSkills}
              renderItem={(skill) => (
                // A plain div, not a `<button>`: the row carries its own write
                // actions (W12), and nesting buttons is invalid HTML.
                <div
                  className="market-card"
                  key={skill.id}
                  onClick={() => void openSkill(skill.id)}
                >
                  <div className="market-card-top">
                    <AvatarBadge name={skill.name} avatarUrl={skill.avatar_url} rootUrl={client?.serverRootUrl} />
                    <div className="market-card-main">
                      <span className="market-card-title">{skill.name}</span>
                      {skill.description && <span className="market-card-sub">{skill.description}</span>}
                    </div>
                    {/* The origin badge is the host's own classification, so a
                        user skill and a marketplace product never look alike
                        (both are `source: custom`). */}
                    <span className="market-tag">{t(originLabelI18nKey(skill.origin))}</span>
                  </div>
                  <Tags tags={[...(skill.required_connectors ?? [])]} />
                  <SkillWriteActionsView
                    skill={skill}
                    busy={skillWrite !== null}
                    onEdit={(id) => setSkillWrite({ kind: "edit", skill: { id, name: skill.name } })}
                    onCopy={(id) => setSkillWrite({ kind: "copy", skill: { id, name: skill.name } })}
                    onDelete={(id) => setSkillWrite({ kind: "delete", skill: { id, name: skill.name } })}
                  />
                </div>
              )}
            />
          </>
        )}

        {surface === "installed" && noun === "connectors" && (
          <MarketGrid
            loading={!client || connectors === null}
            empty={t("catalog.noConnectors")}
            emptyAction={(
              <button className="market-empty-action" type="button" onClick={() => setSurface("browse")}>
                {t("catalog.emptyGoStore")}
              </button>
            )}
            items={visibleConnectors}
            renderItem={(connector) => (
              <button className="market-card" type="button" key={connector.id} onClick={() => void openConnector(connector.id)}>
                <div className="market-card-top">
                  <AvatarBadge name={connector.name} avatarUrl={connector.avatar_url} rootUrl={client?.serverRootUrl} />
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

        {surface === "installed" && noun === "experts" && expertKind === "experts" && (
          <MarketGrid
            loading={!client || agents === null}
            empty={t("catalog.noAgents")}
            emptyAction={(
              <button className="market-empty-action" type="button" onClick={() => setSurface("browse")}>
                {t("catalog.emptyGoStore")}
              </button>
            )}
            items={visibleAgents}
            renderItem={(agent) => (
              <button className="market-card" type="button" key={agent.id} onClick={() => void openAgent(agent.id)}>
                <div className="market-card-top">
                  <AgentBadge agent={agent} />
                  <div className="market-card-main">
                    <span className="market-card-title">{agentDisplayName(agent, lang)}</span>
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

        {/* Installed 专家团. The 精选场景 row lives on the browse surface only —
            repeating it here would show the same teams twice on one tab. */}
        {surface === "installed" && noun === "experts" && expertKind === "teams" && (
          <MarketGrid
            loading={!client || teams === null}
            empty={t("catalog.noTeams")}
            emptyAction={(
              <button className="market-empty-action" type="button" onClick={() => setSurface("browse")}>
                {t("catalog.emptyGoStore")}
              </button>
            )}
            items={visibleTeams}
            renderItem={(team) => (
              <button className="market-card" type="button" key={team.id} onClick={() => void openTeam(team.id)}>
                <div className="market-card-top">
                  <InitialBadge name={team.name} />
                  <div className="market-card-main">
                    <span className="market-card-title">{team.name}</span>
                    <span className="market-card-sub">
                      {team.member_agent_ids.length + 1} {t("catalog.teamMembers")}
                    </span>
                  </div>
                  <ChevronRight size={15} strokeWidth={1.7} className="market-card-arrow" />
                </div>
                <Tags tags={[team.description ?? t("catalog.noDescription")]} />
              </button>
            )}
          />
        )}
      </div>

      {/* Skill write face (W12 / `16` R17): create / edit / copy / delete.
          Mounted once at the view level — the rows only open it. Every write
          re-lists from the host afterwards (`reload`), so the list can never
          show a result the server did not answer with. */}
      <SkillWriteDialogs
        client={client}
        mode={skillWrite}
        onChanged={reload}
        onClose={() => setSkillWrite(null)}
        loadDraft={async (skillId) => {
          // The read face reports the description but deliberately never the
          // full body, so the edit dialog can only prefill what it was given —
          // and the body box stays empty unless the user replaces it.
          const detail = await client?.skills.get(skillId);
          return { description: detail?.description ?? "" };
        }}
      />

      {/* Import dialog — opened by the 技能 tab's 「添加技能」 and the 连接器
          tab's 「自定义连接器」, with the source kind preselected. The panel owns
          the import history and the per-snapshot install state, and renders its
          own detail overlay (which is `position: fixed`, so it covers this
          dialog and closing it returns here). */}
      {importFor && (
        <DialogShell
          onClose={() => setImportFor(null)}
          labelledBy="import-title"
          titleId="import-title"
          title={importFor === "workbuddy-skill-market"
            ? t("catalog.addSkill")
            : t("catalog.customConnector")}
        >
          <ImportPanel initialKind={importFor} />
        </DialogShell>
      )}

      {/* MCP declarations dialog — the same manager the settings dialog hosts,
          opened from the 连接器 tab where the operator is already looking at
          servers. It is a dialog rather than a fourth tab because it configures
          the *host*, not this catalog. */}
      {mcpOpen && <McpManagerDialog onClose={() => setMcpOpen(false)} />}

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
                onToggleEnabled={() => void toggleConnectorEnabled(connectorDetail.id)}
              />
            )}
            {drawer === "agent" && agentDetail && <AgentDrawer detail={agentDetail} />}
            {drawer === "team" && teamDetail && <TeamDrawer detail={teamDetail} />}
            {detailBusy && !storeDrawerItem && !skillDetail && !connectorDetail && !agentDetail && !teamDetail && (
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
  const lang = useLocalizedLang();
  const name = pickLocalized(item.display_name, lang) || item.name;
  const profession = pickLocalized(item.profession, lang);
  const description = pickLocalized(item.display_description, lang) || item.description;
  const tags = pickLocalizedList(item.tags, lang);
  const quickPrompts = item.quick_prompts ?? [];
  return (
    <div className="drawer-body">
      <div className="drawer-head">
        <StoreBadge item={item} size={44} rootUrl={rootUrl} />
        <div>
          <h2>{name}</h2>
          {profession && <div className="drawer-subtitle">{profession}</div>}
          <div className="drawer-chips">
            <span className="market-tag">{storeKindLabel(t, item.kind)}</span>
            <span className="market-tag">v{item.version}</span>
          </div>
        </div>
      </div>
      {description && <p className="drawer-desc">{description}</p>}
      {/* `02` §11.1: stated in full, not just as a hover title — this is the
          reason the install action is missing below. */}
      {item.blocked_reason && <p className="market-block-reason">{item.blocked_reason}</p>}
      {quickPrompts.length > 0 && (
        <div className="drawer-quick-prompts">
          <h3>{t("catalog.agentQuickPrompts")}</h3>
          <ul>
            {quickPrompts.map((prompt, index) => (
              <li key={index}>{pickLocalized(prompt, lang)}</li>
            ))}
          </ul>
        </div>
      )}
      <dl className="market-meta">
        <MetaRow label={t("catalog.fieldVersion")} value={item.version} />
        <MetaRow label={t("catalog.fieldType")} value={storeKindLabel(t, item.kind)} />
        <MetaRow label={t("catalog.fieldTransport")} value={`${marketKindLabel(t, item.source_kind)} · ${item.marketplace_name}`} />
        <MetaList label={t("catalog.fieldSkills")} values={tags} />
      </dl>
      <div className="drawer-actions">
        {item.blocked_reason ? (
          // `02` §11.1: the server refuses this entry, so offering install
          // would only reproduce the refusal.
          <span className="market-tag is-blocked" title={item.blocked_reason}>
            {t("catalog.storeBlocked")}
          </span>
        ) : item.installed ? (
          <span className="market-tag is-status is-success">{t("catalog.storeInstalled")}</span>
        ) : (
          <button className="primary-button" type="button" disabled={busy} onClick={onInstall}>
            {busy ? t("catalog.storeInstalling") : t("catalog.storeInstall")}
          </button>
        )}
        {!item.blocked_reason && item.installed && item.update_available && (
          // There is no update verb on the wire (`store/update-entry` does not
          // exist). This used to be a button that called install, which returns
          // `reused` for an installed entry — a control that did nothing. The
          // note states the path that does work: release it, install again, and
          // the version-aware re-import picks up what the marketplace offers.
          <>
            <span className="market-tag is-status is-warn" title={t("catalog.storeUpdate")}>
              {t("catalog.storeUpdate")}
            </span>
            <span className="settings-row-note">{t("catalog.storeUpdateNote")}</span>
          </>
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
  onToggleEnabled,
}: {
  detail: ConnectorDetail;
  authState?: string;
  busy: boolean;
  onProbe: () => void;
  onAuthStart: () => void;
  onAuthRefresh: () => void;
  onLogout: () => void;
  onToggleEnabled: () => void;
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
        {/* Host-level enable/disable (doc `28` §4.5) — the same action as the
            composer's switch, on the same first-party route. The chip above keeps
            describing the state; this is the control. */}
        <button
          className={`switch-pill ${detail.enabled ? "is-on" : ""}`}
          type="button"
          role="switch"
          aria-checked={detail.enabled}
          aria-label={t("composer.connectorToggleAria", { name: detail.name })}
          title={t("composer.connectorToggleAria", { name: detail.name })}
          disabled={busy}
          onClick={onToggleEnabled}
        >
          <span className="switch-pill-knob" aria-hidden="true" />
        </button>
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
  const lang = useLocalizedLang();
  const displayName = pickLocalized(detail.display_name, lang) || detail.name;
  const profession = pickLocalized(detail.profession, lang);
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
      {(pickLocalized(detail.display_description, lang) || detail.description) && (
        <p className="drawer-desc">{pickLocalized(detail.display_description, lang) || detail.description}</p>
      )}
      {detail.quick_prompts && detail.quick_prompts.length > 0 && (
        <div className="drawer-quick-prompts">
          <h3>{t("catalog.agentQuickPrompts")}</h3>
          <ul>
            {detail.quick_prompts.map((prompt, index) => (
              <li key={index}>{pickLocalized(prompt, lang)}</li>
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


