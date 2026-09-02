/**
 * Agent Store catalog view: Skills + Connectors over the App Server
 * WebSocket link (`src/lib/client.ts`).
 *
 * This view is a pure protocol consumer: every row comes from `skill/list`,
 * `connector/list` and friends. Capabilities are server-driven — the nav only
 * offers entries when `initialize` advertised them.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import {
  ArrowLeft,
  BookOpen,
  Bot,
  ChevronRight,
  CircleAlert,
  Plug,
  RefreshCw,
  Upload,
  Users,
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
  SkillDetail,
  SkillSummary,
  TeamDetail,
  TeamSummary,
} from "../lib/protocol";

type CatalogTab = "skills" | "connectors" | "agents" | "teams" | "imports";

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

function connectorStateLabel(t: (key: string, opts?: Record<string, unknown>) => string, status: ConnectorStatus | string): string {
  const key = CONNECTOR_STATE_KEYS[status];
  return key ? t(key) : t("catalog.stateOther", { status });
}

function authStateLabel(t: (key: string, opts?: Record<string, unknown>) => string, state: string): string {
  const key = AUTH_STATE_KEYS[state];
  return key ? t(key) : t("catalog.authOther", { status: state });
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

export function CatalogView() {
  const { t } = useTranslation();
  const client = useAppStore((s) => s.client);
  const capabilities = useAppStore((s) => s.client?.initializeInfo?.capabilities ?? null);
  const onBack = useAppStore((s) => s.toggleCatalog);
  const [tab, setTab] = useState<CatalogTab>("skills");
  const [skills, setSkills] = useState<SkillSummary[] | null>(null);
  const [connectors, setConnectors] = useState<ConnectorSummary[] | null>(null);
  const [skillDetail, setSkillDetail] = useState<SkillDetail | null>(null);
  const [connectorDetail, setConnectorDetail] = useState<ConnectorDetail | null>(null);
  // Agent / Team / Importer catalog (agents/teams over WS, imports over HTTP)
  const [agents, setAgents] = useState<AgentSummary[] | null>(null);
  const [teams, setTeams] = useState<TeamSummary[] | null>(null);
  const [agentDetail, setAgentDetail] = useState<AgentDetail | null>(null);
  const [teamDetail, setTeamDetail] = useState<TeamDetail | null>(null);
  const [imports, setImports] = useState<ImportSummary[] | null>(null);
  const [importDetail, setImportDetail] = useState<ImportDetail | null>(null);
  const [importResult, setImportResult] = useState<ImportResult | null>(null);
  const [importPath, setImportPath] = useState("");
  const [importKind, setImportKind] = useState<ImportSourceKind>("codebuddy-plugin");
  const [importBusy, setImportBusy] = useState(false);
  const [detailBusy, setDetailBusy] = useState<string | null>(null);
  const [authMap, setAuthMap] = useState<Map<string, string>>(new Map());
  const [error, setError] = useState<string | null>(null);
  /** Localize a caught error and append a retryable hint without baking
   *  wording into `lib/errors`. */
  const reportError = (caught: unknown) => {
    setError(formatError(caught) + (isRetryableError(caught) ? t("common.retryable") : ""));
  };
  const [reloadTick, setReloadTick] = useState(0);
  const activeRef = useRef(true);

  useEffect(() => {
    activeRef.current = true;
    return () => {
      activeRef.current = false;
    };
  }, []);

  const reload = useCallback(() => {
    setSkills(null);
    setConnectors(null);
    setAgents(null);
    setTeams(null);
    setImports(null);
    setError(null);
    setReloadTick((tick) => tick + 1);
  }, []);

  useEffect(() => {
    if (!client) return;
    let cancelled = false;
    const load = async () => {
      try {
        const [skillList, connectorList, agentList, teamList, importList] = await Promise.all([
          capabilities?.skills ? client.skills.list() : Promise.resolve([]),
          capabilities?.connectors ? client.connectors.list() : Promise.resolve([]),
          capabilities?.agents ? client.agents.list() : Promise.resolve([]),
          capabilities?.teams ? client.teams.list() : Promise.resolve([]),
          capabilities?.imports ? client.listImports() : Promise.resolve([]),
        ]);
        if (cancelled || !activeRef.current) return;
        setSkills(skillList);
        setConnectors(connectorList);
        setAgents(agentList);
        setTeams(teamList);
        setImports(importList);
      } catch (caught) {
        if (cancelled || !activeRef.current) return;
        reportError(caught);
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
    setDetailBusy(skillId);
    const previous = skillDetail;
    setSkillDetail(null);
    try {
      const detail = await client.skills.get(skillId);
      if (!activeRef.current) return;
      setSkillDetail(detail);
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
      setSkillDetail(previous);
    } finally {
      if (activeRef.current) setDetailBusy(null);
    }
  }, [client, skillDetail]);

  const openConnector = useCallback(async (connectorId: string) => {
    if (!client) return;
    setDetailBusy(connectorId);
    const previous = connectorDetail;
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
      setConnectorDetail(previous);
    } finally {
      if (activeRef.current) setDetailBusy(null);
    }
  }, [client, connectorDetail]);

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
  }, [client]);

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
  }, [client, refreshAuth]);

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
    setDetailBusy(agentId);
    const previous = agentDetail;
    setAgentDetail(null);
    try {
      const detail = await client.agents.get(agentId);
      if (!activeRef.current) return;
      setAgentDetail(detail);
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
      setAgentDetail(previous);
    } finally {
      if (activeRef.current) setDetailBusy(null);
    }
  }, [client, agentDetail]);

  const openTeam = useCallback(async (teamId: string) => {
    if (!client) return;
    setDetailBusy(teamId);
    const previous = teamDetail;
    setTeamDetail(null);
    try {
      const detail = await client.teams.get(teamId);
      if (!activeRef.current) return;
      setTeamDetail(detail);
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
      setTeamDetail(previous);
    } finally {
      if (activeRef.current) setDetailBusy(null);
    }
  }, [client, teamDetail]);

  const runImport = useCallback(async () => {
    if (!client || importPath.trim().length === 0) {
      setError(t("catalog.importEmptySource"));
      return;
    }
    setImportBusy(true);
    setImportResult(null);
    setImportDetail(null);
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

  const openImport = useCallback(async (snapshotId: string) => {
    if (!client) return;
    setDetailBusy(snapshotId);
    const previous = importDetail;
    setImportDetail(null);
    try {
      const detail = await client.getImport(snapshotId);
      if (!activeRef.current) return;
      setImportDetail(detail);
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
      setImportDetail(previous);
    } finally {
      if (activeRef.current) setDetailBusy(null);
    }
  }, [client, importDetail]);

  return (
    <section className="catalog-view" aria-label={t("catalog.ariaLabel")}>
      <header className="catalog-topbar">
        <div className="topbar-left">
          <IconButton label={t("catalog.backToChat")} onClick={onBack} className="catalog-back"><ArrowLeft size={19} strokeWidth={1.7} /></IconButton>
          <BookOpen aria-hidden="true" className="catalog-glyph" size={19} strokeWidth={1.7} />
          <div className="thread-heading"><span>{t("catalog.title")}</span><small>{t("catalog.subtitle")}</small></div>
        </div>
        <div className="topbar-actions">
          <button className="quiet-button" type="button" onClick={reload} title={t("catalog.reloadTitle")}>
            <RefreshCw size={15} strokeWidth={1.7} /> {t("catalog.reload")}
          </button>
        </div>
      </header>

      <div className="catalog-tabs" role="tablist" aria-label={t("catalog.typeLabel")}>
        {capabilities?.skills !== false && (
          <button
            className={`catalog-tab ${tab === "skills" ? "is-active" : ""}`}
            type="button"
            role="tab"
            aria-selected={tab === "skills"}
            onClick={() => { setTab("skills"); setSkillDetail(null); }}
          >
            <BookOpen size={15} strokeWidth={1.7} /> {t("catalog.tabSkills")}
          </button>
        )}
        {capabilities?.connectors !== false && (
          <button
            className={`catalog-tab ${tab === "connectors" ? "is-active" : ""}`}
            type="button"
            role="tab"
            aria-selected={tab === "connectors"}
            onClick={() => { setTab("connectors"); setConnectorDetail(null); }}
          >
            <Plug size={15} strokeWidth={1.7} /> {t("catalog.tabConnectors")}
          </button>
        )}
        {capabilities?.agents !== false && (
          <button
            className={`catalog-tab ${tab === "agents" ? "is-active" : ""}`}
            type="button"
            role="tab"
            aria-selected={tab === "agents"}
            onClick={() => { setTab("agents"); setAgentDetail(null); }}
          >
            <Bot size={15} strokeWidth={1.7} /> {t("catalog.tabAgents")}
          </button>
        )}
        {capabilities?.teams !== false && (
          <button
            className={`catalog-tab ${tab === "teams" ? "is-active" : ""}`}
            type="button"
            role="tab"
            aria-selected={tab === "teams"}
            onClick={() => { setTab("teams"); setTeamDetail(null); }}
          >
            <Users size={15} strokeWidth={1.7} /> {t("catalog.tabTeams")}
          </button>
        )}
        {capabilities?.imports !== false && (
          <button
            className={`catalog-tab ${tab === "imports" ? "is-active" : ""}`}
            type="button"
            role="tab"
            aria-selected={tab === "imports"}
            onClick={() => { setTab("imports"); setImportDetail(null); }}
          >
            <Upload size={15} strokeWidth={1.7} /> {t("catalog.tabImports")}
          </button>
        )}
      </div>

      {error && <div className="catalog-alert" role="alert"><CircleAlert size={15} strokeWidth={1.8} /><span>{error}</span><button type="button" onClick={() => setError(null)} aria-label={t("common.closeError")}>✕</button></div>}

      <div className="catalog-body">
        {tab === "skills" && (
          <div className="catalog-layout">
            <div className="catalog-list">
              {!client && <p className="catalog-empty">请先连接 App Server。</p>}
              {client && skills === null && <p className="catalog-empty">{t("catalog.loadingSkills")}</p>}
              {client && skills !== null && skills.length === 0 && (
                <p className="catalog-empty">{t("catalog.noSkills")}</p>
              )}
              {skills?.map((skill) => (
                <button
                  className={`catalog-row ${skillDetail?.id === skill.id ? "is-active" : ""}`}
                  type="button"
                  key={skill.id}
                  onClick={() => void openSkill(skill.id)}
                >
                  <div className="catalog-row-main">
                    <span className="catalog-row-title">{skill.name}</span>
                    {skill.description && <span className="catalog-row-sub">{skill.description}</span>}
                  </div>
                  <div className="catalog-row-meta">
                    <span className="source-chip">{skill.source}</span>
                    <span className="compat-chip">{skill.compatibility_status}</span>
                    {detailBusy === skill.id && <span className="row-busy">{t("catalog.loading")}</span>}
                    <ChevronRight size={15} strokeWidth={1.7} />
                  </div>
                </button>
              ))}
            </div>
            <div className="catalog-detail">
              {skillDetail ? (
                <SkillDetailPane detail={skillDetail} />
              ) : (
                <p className="catalog-detail-empty">{t("catalog.selectSkill")}</p>
              )}
            </div>
          </div>
        )}

        {tab === "connectors" && (
          <div className="catalog-layout">
            <div className="catalog-list">
              {!client && <p className="catalog-empty">{t("catalog.connectFirst")}</p>}
              {client && connectors === null && <p className="catalog-empty">{t("catalog.loadingConnectors")}</p>}
              {client && connectors !== null && connectors.length === 0 && (
                <p className="catalog-empty">{t("catalog.noConnectors")}</p>
              )}
              {connectors?.map((connector) => (
                <button
                  className={`catalog-row ${connectorDetail?.id === connector.id ? "is-active" : ""}`}
                  type="button"
                  key={connector.id}
                  onClick={() => void openConnector(connector.id)}
                >
                  <div className="catalog-row-main">
                    <span className="catalog-row-title">{connector.name}</span>
                    <span className="catalog-row-sub">
                      {connector.kind} · {connector.transport_summary}
                    </span>
                  </div>
                  <div className="catalog-row-meta">
                    <span className={`status-badge ${connectorStateClass(connector.status)}`}>
                      {connectorStateLabel(t, connector.status)}
                    </span>
                    {detailBusy === connector.id && <span className="row-busy">加载中…</span>}
                    <ChevronRight size={15} strokeWidth={1.7} />
                  </div>
                </button>
              ))}
            </div>
            <div className="catalog-detail">
              {connectorDetail ? (
                <ConnectorDetailPane
                  detail={connectorDetail}
                  authState={authMap.get(connectorDetail.id)}
                  busy={detailBusy === connectorDetail.id}
                  onProbe={() => void probeConnector(connectorDetail.id)}
                  onAuthStart={() => void startAuth(connectorDetail.id)}
                  onAuthRefresh={() => void refreshAuth(connectorDetail.id)}
                  onLogout={() => void logoutConnector(connectorDetail.id)}
                />
              ) : (
                <p className="catalog-detail-empty">{t("catalog.selectConnector")}</p>
              )}
            </div>
          </div>
        )}

        {tab === "agents" && (
          <div className="catalog-layout">
            <div className="catalog-list">
              {!client && <p className="catalog-empty">{t("catalog.connectFirst")}</p>}
              {client && agents === null && <p className="catalog-empty">{t("catalog.loadingAgents")}</p>}
              {client && agents !== null && agents.length === 0 && (
                <p className="catalog-empty">{t("catalog.noAgents")}</p>
              )}
              {agents?.map((agent) => (
                <button
                  className={`catalog-row ${agentDetail?.id === agent.id ? "is-active" : ""}`}
                  type="button"
                  key={agent.id}
                  onClick={() => void openAgent(agent.id)}
                >
                  <div className="catalog-row-main">
                    <span className="catalog-row-title">{agent.name}</span>
                    {agent.description && <span className="catalog-row-sub">{agent.description}</span>}
                  </div>
                  <div className="catalog-row-meta">
                    <span className="source-chip">{agent.source}</span>
                    <span className="compat-chip">{agent.compatibility_status}</span>
                    {detailBusy === agent.id && <span className="row-busy">{t("catalog.loading")}</span>}
                    <ChevronRight size={15} strokeWidth={1.7} />
                  </div>
                </button>
              ))}
            </div>
            <div className="catalog-detail">
              {agentDetail ? (
                <AgentDetailPane detail={agentDetail} />
              ) : (
                <p className="catalog-detail-empty">{t("catalog.selectAgent")}</p>
              )}
            </div>
          </div>
        )}

        {tab === "teams" && (
          <div className="catalog-layout">
            <div className="catalog-list">
              {!client && <p className="catalog-empty">{t("catalog.connectFirst")}</p>}
              {client && teams === null && <p className="catalog-empty">{t("catalog.loadingTeams")}</p>}
              {client && teams !== null && teams.length === 0 && (
                <p className="catalog-empty">{t("catalog.noTeams")}</p>
              )}
              {teams?.map((team) => (
                <button
                  className={`catalog-row ${teamDetail?.id === team.id ? "is-active" : ""}`}
                  type="button"
                  key={team.id}
                  onClick={() => void openTeam(team.id)}
                >
                  <div className="catalog-row-main">
                    <span className="catalog-row-title">{team.name}</span>
                    {team.description && <span className="catalog-row-sub">{team.description}</span>}
                  </div>
                  <div className="catalog-row-meta">
                    <span className="source-chip">{team.source}</span>
                    <span className="compat-chip">{team.compatibility_status}</span>
                    <span className="catalog-row-sub">{team.member_agent_ids.length + 1} {t("catalog.teamMembers")}</span>
                    {detailBusy === team.id && <span className="row-busy">{t("catalog.loading")}</span>}
                    <ChevronRight size={15} strokeWidth={1.7} />
                  </div>
                </button>
              ))}
            </div>
            <div className="catalog-detail">
              {teamDetail ? (
                <TeamDetailPane detail={teamDetail} />
              ) : (
                <p className="catalog-detail-empty">{t("catalog.selectTeam")}</p>
              )}
            </div>
          </div>
        )}

        {tab === "imports" && (
          <div className="catalog-layout catalog-layout-imports">
            <div className="catalog-list">
              <div className="import-form">
                <label htmlFor="import-path">{t("catalog.importPath")}</label>
                <div className="import-form-row">
                  <select
                    id="import-kind"
                    className="import-kind"
                    value={importKind}
                    onChange={(event) => setImportKind(event.target.value as ImportSourceKind)}
                    aria-label={t("catalog.importSource")}
                  >
                    <option value="codebuddy-plugin">{t("catalog.importKindPlugin")}</option>
                    <option value="workbuddy-skill-market">{t("catalog.importKindSkills")}</option>
                    <option value="workbuddy-connector-market">{t("catalog.importKindConnectors")}</option>
                  </select>
                  <input
                    id="import-path"
                    className="import-path"
                    type="text"
                    placeholder={t("catalog.importPathPlaceholder")}
                    value={importPath}
                    onChange={(event) => setImportPath(event.target.value)}
                    onKeyDown={(event) => { if (event.key === "Enter") void runImport(); }}
                  />
                  <button
                    className="primary-button"
                    type="button"
                    disabled={importBusy}
                    onClick={() => void runImport()}
                  >
                    {importBusy ? t("catalog.importing") : t("catalog.importRun")}
                  </button>
                </div>
              </div>

              {importResult && (
                <ImportResultCard
                  result={importResult}
                  onOpen={() => void openImport(importResult.snapshot_id)}
                />
              )}

              {!client && <p className="catalog-empty">{t("catalog.connectFirst")}</p>}
              {client && imports === null && <p className="catalog-empty">{t("catalog.loadingImports")}</p>}
              {client && imports !== null && imports.length === 0 && (
                <p className="catalog-empty">{t("catalog.noImports")}</p>
              )}
              {imports?.map((item) => (
                <button
                  className={`catalog-row ${importDetail?.snapshot_id === item.snapshot_id ? "is-active" : ""}`}
                  type="button"
                  key={item.snapshot_id}
                  onClick={() => void openImport(item.snapshot_id)}
                >
                  <div className="catalog-row-main">
                    <span className="catalog-row-title">{item.name} <small>v{item.version}</small></span>
                    <span className="catalog-row-sub">
                      {item.source_kind} · {t("catalog.importComponents", { count: item.component_count })}
                    </span>
                  </div>
                  <div className="catalog-row-meta">
                    <span className={`import-status ${importStatusClass(item.status)}`}>{importStatusLabel(t, item.status)}</span>
                    {detailBusy === item.snapshot_id && <span className="row-busy">{t("catalog.loading")}</span>}
                    <ChevronRight size={15} strokeWidth={1.7} />
                  </div>
                </button>
              ))}
            </div>
            <div className="catalog-detail">
              {importDetail ? (
                <ImportDetailPane detail={importDetail} />
              ) : (
                <p className="catalog-detail-empty">{t("catalog.selectImport")}</p>
              )}
            </div>
          </div>
        )}
      </div>
    </section>
  );
}

function IconButton({
  label,
  className = "",
  children,
  onClick,
  ...props
}: {
  label: string;
  className?: string;
  children: React.ReactNode;
  onClick?: () => void;
} & Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, "children" | "aria-label" | "className" | "onClick">) {
  return <button type="button" className={`icon-button ${className}`} aria-label={label} title={label} onClick={onClick} {...props}>{children}</button>;
}

function SkillDetailPane({ detail }: { detail: SkillDetail }) {
  const { t } = useTranslation();
  return (
    <div className="catalog-detail-card">
      <div className="catalog-detail-head">
        <h2>{detail.name}</h2>
        <span className="source-chip">{detail.source}</span>
        <span className="compat-chip">{detail.compatibility_status}</span>
      </div>
      {detail.description && <p className="catalog-detail-desc">{detail.description}</p>}
      <dl className="catalog-detail-meta">
        <div><dt>{t("catalog.fieldMode")}</dt><dd>{detail.mode}</dd></div>
        <div><dt>{t("catalog.fieldInvocation")}</dt><dd>{detail.invocation_policy}</dd></div>
        <div><dt>{t("catalog.fieldVersion")}</dt><dd>{detail.version}</dd></div>
        {detail.required_connectors?.length > 0 && (
          <div><dt>{t("catalog.fieldRequiredConnectors")}</dt><dd>{(detail.required_connectors ?? []).join(", ")}</dd></div>
        )}
      </dl>
      {detail.instructions_summary && (
        <details className="catalog-instructions">
          <summary>{t("catalog.fieldInstructions")}</summary>
          <pre>{detail.instructions_summary}</pre>
        </details>
      )}
    </div>
  );
}

function ConnectorDetailPane({
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
  const authenticated = authState === "authenticated";
  const { t } = useTranslation();
  return (
    <div className="catalog-detail-card">
      <div className="catalog-detail-head">
        <h2>{detail.name}</h2>
        <span className={`status-badge ${connectorStateClass(detail.status)}`}>
          {connectorStateLabel(t, detail.status)}
        </span>
        {!detail.enabled && <span className="status-badge is-warn">{t("catalog.disabled")}</span>}
      </div>
      {detail.description && <p className="catalog-detail-desc">{detail.description}</p>}
      <dl className="catalog-detail-meta">
        <div><dt>{t("catalog.fieldType")}</dt><dd>{detail.kind}</dd></div>
        <div><dt>{t("catalog.fieldTransport")}</dt><dd>{detail.transport_summary}</dd></div>
        <div><dt>{t("catalog.fieldAuth")}</dt><dd>{detail.auth_mode}</dd></div>
        <div><dt>{t("catalog.fieldNamespace")}</dt><dd>{detail.tool_filter ?? "—"}</dd></div>
      </dl>

      {detail.auth_mode === "oauth" ? (
        <div className="catalog-auth-row">
          <span className={`status-badge ${authenticated ? "is-success" : "is-warn"}`}>
            {authStateLabel(t, authState ?? "not_authenticated")}
          </span>
          {!authenticated
            ? <button className="quiet-button" type="button" onClick={onAuthStart} disabled={busy}>{t("catalog.authAuthorize")}</button>
            : <button className="quiet-button" type="button" onClick={onLogout} disabled={busy}>{t("catalog.authRevoke")}</button>}
          <button className="quiet-button" type="button" onClick={onAuthRefresh} disabled={busy}>{t("catalog.authRefreshStatus")}</button>
          <button className="primary-button" type="button" onClick={onProbe} disabled={busy}>{t("catalog.authTest")}</button>
        </div>
      ) : (
        <div className="catalog-auth-row">
          <button className="primary-button" type="button" onClick={onProbe} disabled={busy}>{t("catalog.authTest")}</button>
        </div>
      )}
      {busy && <p className="catalog-detail-hint">{t("common.processing")}</p>}

      <details className="catalog-tools" open={(detail.tools?.length ?? 0) > 0}>
        <summary>{t("catalog.tools", { count: detail.tools?.length ?? 0 })}</summary>
        {(detail.tools ?? []).length === 0 ? (
          <p className="catalog-detail-hint">{t("catalog.noToolsHint")}</p>
        ) : (
          <ul className="catalog-tool-list">
            {(detail.tools ?? []).map((tool) => (
              <li key={tool.name}>
                <span className="tool-name">{tool.name}</span>
                {tool.description && <span className="tool-desc">{tool.description}</span>}
              </li>
            ))}
          </ul>
        )}
      </details>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Agent / Team / Importer panes
// ---------------------------------------------------------------------------

const IMPORT_STATUS_KEYS: Record<string, string> = {
  completed: "catalog.importStatusCompleted",
  "completed-with-warnings": "catalog.importStatusWarnings",
  blocked: "catalog.importStatusBlocked",
  failed: "catalog.importStatusFailed",
};

function importStatusLabel(t: (key: string, opts?: Record<string, unknown>) => string, status: string): string {
  const key = IMPORT_STATUS_KEYS[status];
  return key ? t(key) : t("catalog.stateOther", { status });
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

function semanticLabel(t: (key: string, opts?: Record<string, unknown>) => string, status: string): string {
  const key = `catalog.semantic_${status}`;
  const label = t(key) as string;
  return label.startsWith("catalog.") ? status : label;
}

/** Three-dimensional compatibility report chips (03 §1). */
function CompatTripleChips({ triple, t }: { triple: CompatibilityTriple; t: (key: string, opts?: Record<string, unknown>) => string }) {
  if (!triple) return null;
  return (
    <div className="compat-triple">
      <span className="compat-chip">{semanticLabel(t, triple.semantic_status)}</span>
      <span className={`compat-chip ${triple.runtime_status === "not-verified" ? "is-warn" : ""}`}>
        {t("catalog.compatRuntime")}: {triple.runtime_status}
      </span>
      <span className="compat-chip">{t("catalog.compatDistribution")}: {triple.distribution_status}</span>
      {triple.reasons.length > 0 && (
        <details className="compat-reasons">
          <summary>{t("catalog.compatReasons")}</summary>
          <ul>
            {triple.reasons.map((reason, index) => <li key={index}>{reason}</li>)}
          </ul>
        </details>
      )}
    </div>
  );
}

function AgentDetailPane({ detail }: { detail: AgentDetail }) {
  const { t } = useTranslation();
  return (
    <div className="catalog-detail-card">
      <div className="catalog-detail-head">
        <h2>{detail.name}</h2>
        <span className="source-chip">{detail.source}</span>
        <span className="compat-chip">{detail.compatibility_status}</span>
      </div>
      {detail.description && <p className="catalog-detail-desc">{detail.description}</p>}
      <dl className="catalog-detail-meta">
        <div><dt>{t("catalog.fieldVersion")}</dt><dd>{detail.version}</dd></div>
        <div><dt>{t("catalog.fieldModel")}</dt><dd>{detail.model_summary ?? "—"}</dd></div>
        {detail.effort && <div><dt>{t("catalog.fieldEffort")}</dt><dd>{detail.effort}</dd></div>}
        {detail.max_turns != null && <div><dt>{t("catalog.fieldMaxTurns")}</dt><dd>{detail.max_turns}</dd></div>}
        {detail.tool_policy_summary && <div><dt>{t("catalog.fieldToolPolicy")}</dt><dd>{detail.tool_policy_summary}</dd></div>}
        {(detail.disallowed_tools ?? []).length > 0 && (
          <div><dt>{t("catalog.fieldDisallowedTools")}</dt><dd>{(detail.disallowed_tools ?? []).join(", ")}</dd></div>
        )}
        {(detail.skills ?? []).length > 0 && (
          <div><dt>{t("catalog.fieldSkills")}</dt><dd>{(detail.skills ?? []).join(", ")}</dd></div>
        )}
        {detail.memory && <div><dt>{t("catalog.fieldMemory")}</dt><dd>{detail.memory}</dd></div>}
        {detail.background && <div><dt>{t("catalog.fieldBackground")}</dt><dd>{detail.background}</dd></div>}
        {detail.isolation && <div><dt>{t("catalog.fieldIsolation")}</dt><dd>{detail.isolation}</dd></div>}
      </dl>
      {detail.permission_mode_ignored && (
        <p className="catalog-detail-hint">{t("catalog.agentPermissionIgnored")}</p>
      )}
    </div>
  );
}

function TeamDetailPane({ detail }: { detail: TeamDetail }) {
  const { t } = useTranslation();
  return (
    <div className="catalog-detail-card">
      <div className="catalog-detail-head">
        <h2>{detail.name}</h2>
        <span className="source-chip">{detail.source}</span>
        <span className="compat-chip">{detail.compatibility_status}</span>
      </div>
      {detail.description && <p className="catalog-detail-desc">{detail.description}</p>}
      <dl className="catalog-detail-meta">
        <div><dt>{t("catalog.fieldVersion")}</dt><dd>{detail.version}</dd></div>
        <div><dt>{t("catalog.fieldLead")}</dt><dd>{detail.lead_agent_id}</dd></div>
        <div><dt>{t("catalog.fieldMembers")}</dt><dd>{detail.member_agent_ids.join(", ")}</dd></div>
        <div><dt>{t("catalog.fieldPlanner")}</dt><dd>{detail.planner_policy}</dd></div>
        <div><dt>{t("catalog.fieldWorkflowLimits")}</dt><dd>{JSON.stringify(detail.workflow_limits)}</dd></div>
        <div><dt>{t("catalog.fieldCapabilities")}</dt><dd>{detail.team_runtime_capabilities.join(", ")}</dd></div>
      </dl>
    </div>
  );
}

function ImportResultCard({ result, onOpen }: { result: ImportResult; onOpen: () => void }) {
  const { t } = useTranslation();
  return (
    <div className="import-result">
      <div className="import-result-head">
        <span className={`import-status ${importStatusClass(result.status)}`}>
          {importStatusLabel(t, result.status)}
        </span>
        <span className="catalog-row-title">{result.name} <small>v{result.version}</small></span>
        {result.reused && <span className="source-chip">{t("catalog.importReused")}</span>}
      </div>
      <dl className="catalog-detail-meta">
        <div><dt>{t("catalog.importDigest")}</dt><dd className="import-digest">{result.content_digest}</dd></div>
        <div><dt>{t("catalog.importComponents", { count: result.component_count })}</dt><dd>{result.component_count}</dd></div>
      </dl>
      <CompatTripleChips triple={result.component_status} t={t} />
      {result.warnings.length > 0 && (
        <details className="compat-reasons"><summary>{t("catalog.importWarnings", { count: result.warnings.length })}</summary>
          <ul>{result.warnings.map((warning, index) => <li key={index}>{warning}</li>)}</ul>
        </details>
      )}
      {result.errors.length > 0 && (
        <details className="compat-reasons" open><summary>{t("catalog.importErrors", { count: result.errors.length })}</summary>
          <ul>{result.errors.map((error, index) => <li key={index}>{error}</li>)}</ul>
        </details>
      )}
      <button className="quiet-button" type="button" onClick={onOpen}>{t("catalog.importOpenDetail")}</button>
    </div>
  );
}

function ImportDetailPane({ detail }: { detail: ImportDetail }) {
  const { t } = useTranslation();
  return (
    <div className="catalog-detail-card">
      <div className="catalog-detail-head">
        <h2>{detail.name} <small>v{detail.version}</small></h2>
        <span className={`import-status ${importStatusClass(detail.status)}`}>
          {importStatusLabel(t, detail.status)}
        </span>
      </div>
      <dl className="catalog-detail-meta">
        <div><dt>{t("catalog.fieldVersion")}</dt><dd>{detail.version}</dd></div>
        <div><dt>{t("catalog.importDigest")}</dt><dd className="import-digest">{detail.content_digest}</dd></div>
        <div><dt>{t("catalog.importComponents", { count: detail.components.length })}</dt><dd>{detail.components.length}</dd></div>
      </dl>
      <CompatTripleChips triple={detail.component_status} t={t} />
      {detail.warnings.length > 0 && (
        <details className="compat-reasons"><summary>{t("catalog.importWarnings", { count: detail.warnings.length })}</summary>
          <ul>{detail.warnings.map((warning, index) => <li key={index}>{warning}</li>)}</ul>
        </details>
      )}
      {detail.errors.length > 0 && (
        <details className="compat-reasons" open><summary>{t("catalog.importErrors", { count: detail.errors.length })}</summary>
          <ul>{detail.errors.map((error, index) => <li key={index}>{error}</li>)}</ul>
        </details>
      )}
      <details className="catalog-tools" open>
        <summary>{t("catalog.importComponents", { count: detail.components.length })}</summary>
        <ul className="catalog-tool-list">
          {detail.components.map((component) => (
            <li key={component.id} className="import-component">
              <span className="import-component-main">
                <span className="tool-name">{component.kind}: {component.name}</span>
                <CompatTripleChips triple={component.compatibility} t={t} />
              </span>
            </li>
          ))}
        </ul>
      </details>
    </div>
  );
}