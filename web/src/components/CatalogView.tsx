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
  ChevronRight,
  CircleAlert,
  Plug,
  RefreshCw,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { formatError, isRetryableError } from "../lib/errors";
import { useAppStore } from "../store/appStore";
import type {
  ConnectorDetail,
  ConnectorStatus,
  ConnectorSummary,
  SkillDetail,
  SkillSummary,
} from "../lib/protocol";

type CatalogTab = "skills" | "connectors";

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
    setError(null);
    setReloadTick((tick) => tick + 1);
  }, []);

  useEffect(() => {
    if (!client) return;
    let cancelled = false;
    const load = async () => {
      try {
        const [skillList, connectorList] = await Promise.all([
          capabilities?.skills ? client.skills.list() : Promise.resolve([]),
          capabilities?.connectors ? client.connectors.list() : Promise.resolve([]),
        ]);
        if (cancelled || !activeRef.current) return;
        setSkills(skillList);
        setConnectors(connectorList);
      } catch (caught) {
        if (cancelled || !activeRef.current) return;
        reportError(caught);
        setSkills(null);
        setConnectors(null);
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