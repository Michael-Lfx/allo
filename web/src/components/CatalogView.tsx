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
import type { AppServerClient } from "../lib/client";
import { isAppServerError } from "../lib/errors";
import type {
  Capabilities,
  ConnectorDetail,
  ConnectorStatus,
  ConnectorSummary,
  SkillDetail,
  SkillSummary,
} from "../lib/protocol";

type CatalogTab = "skills" | "connectors";

const CONNECTOR_STATE_LABEL: Record<string, string> = {
  installed: "已安装",
  configured: "已配置",
  authorization_required: "需要授权",
  authenticated: "已授权",
  connected: "已连接",
  degraded: "降级",
  error: "错误",
  reauthorization_required: "需重新授权",
};

const AUTH_STATE_LABEL: Record<string, string> = {
  authenticated: "已授权",
  not_authenticated: "未授权",
  reauthorization_required: "需重新授权",
};

function errorMessage(error: unknown): string {
  if (isAppServerError(error)) {
    return `[${error.code}] ${error.message}`;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
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

export function CatalogView({
  client,
  capabilities,
  onBack,
}: {
  client: AppServerClient | null;
  capabilities: Capabilities | null;
  onBack: () => void;
}) {
  const [tab, setTab] = useState<CatalogTab>("skills");
  const [skills, setSkills] = useState<SkillSummary[] | null>(null);
  const [connectors, setConnectors] = useState<ConnectorSummary[] | null>(null);
  const [skillDetail, setSkillDetail] = useState<SkillDetail | null>(null);
  const [connectorDetail, setConnectorDetail] = useState<ConnectorDetail | null>(null);
  const [detailBusy, setDetailBusy] = useState<string | null>(null);
  const [authMap, setAuthMap] = useState<Map<string, string>>(new Map());
  const [error, setError] = useState<string | null>(null);
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
        setError(errorMessage(caught));
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
      setError(errorMessage(caught));
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
      setError(errorMessage(caught));
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
      setError(result.success ? null : `探测失败：${result.error ?? result.code ?? "未知错误"}`);
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
      setError(errorMessage(caught));
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
      setError(errorMessage(caught));
    }
  }, [client]);

  const startAuth = useCallback(async (connectorId: string) => {
    if (!client) return;
    setDetailBusy(connectorId);
    try {
      const started = await client.connectors.authStart(connectorId);
      if (!activeRef.current) return;
      if (started.error) {
        setError(`授权启动失败：${started.error}`);
        return;
      }
      setError("授权已在可信主机上启动，请在弹出的浏览器窗口中完成授权。");
      await refreshAuth(connectorId);
    } catch (caught) {
      if (!activeRef.current) return;
      setError(errorMessage(caught));
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
      setError(errorMessage(caught));
    } finally {
      if (activeRef.current) setDetailBusy(null);
    }
  }, [client]);

  return (
    <section className="catalog-view" aria-label="Agent Store 目录">
      <header className="catalog-topbar">
        <div className="topbar-left">
          <IconButton label="返回聊天" onClick={onBack} className="catalog-back"><ArrowLeft size={19} strokeWidth={1.7} /></IconButton>
          <BookOpen aria-hidden="true" className="catalog-glyph" size={19} strokeWidth={1.7} />
          <div className="thread-heading"><span>技能与连接器</span><small>Agent Store 目录</small></div>
        </div>
        <div className="topbar-actions">
          <button className="quiet-button" type="button" onClick={reload} title="重新加载目录">
            <RefreshCw size={15} strokeWidth={1.7} /> 刷新
          </button>
        </div>
      </header>

      <div className="catalog-tabs" role="tablist" aria-label="目录类型">
        {capabilities?.skills !== false && (
          <button
            className={`catalog-tab ${tab === "skills" ? "is-active" : ""}`}
            type="button"
            role="tab"
            aria-selected={tab === "skills"}
            onClick={() => { setTab("skills"); setSkillDetail(null); }}
          >
            <BookOpen size={15} strokeWidth={1.7} /> 技能
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
            <Plug size={15} strokeWidth={1.7} /> 连接器
          </button>
        )}
      </div>

      {error && <div className="catalog-alert" role="alert"><CircleAlert size={15} strokeWidth={1.8} /><span>{error}</span><button type="button" onClick={() => setError(null)} aria-label="关闭错误">✕</button></div>}

      <div className="catalog-body">
        {tab === "skills" && (
          <div className="catalog-layout">
            <div className="catalog-list">
              {!client && <p className="catalog-empty">请先连接 App Server。</p>}
              {client && skills === null && <p className="catalog-empty">正在加载技能目录…</p>}
              {client && skills !== null && skills.length === 0 && (
                <p className="catalog-empty">没有可用的技能。</p>
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
                    {detailBusy === skill.id && <span className="row-busy">加载中…</span>}
                    <ChevronRight size={15} strokeWidth={1.7} />
                  </div>
                </button>
              ))}
            </div>
            <div className="catalog-detail">
              {skillDetail ? (
                <SkillDetailPane detail={skillDetail} />
              ) : (
                <p className="catalog-detail-empty">选择一个技能查看详情。</p>
              )}
            </div>
          </div>
        )}

        {tab === "connectors" && (
          <div className="catalog-layout">
            <div className="catalog-list">
              {!client && <p className="catalog-empty">请先连接 App Server。</p>}
              {client && connectors === null && <p className="catalog-empty">正在加载连接器目录…</p>}
              {client && connectors !== null && connectors.length === 0 && (
                <p className="catalog-empty">没有可用的连接器。</p>
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
                      {CONNECTOR_STATE_LABEL[connector.status] ?? connector.status}
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
                <p className="catalog-detail-empty">选择一个连接器查看状态与工具。</p>
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
  return (
    <div className="catalog-detail-card">
      <div className="catalog-detail-head">
        <h2>{detail.name}</h2>
        <span className="source-chip">{detail.source}</span>
        <span className="compat-chip">{detail.compatibility_status}</span>
      </div>
      {detail.description && <p className="catalog-detail-desc">{detail.description}</p>}
      <dl className="catalog-detail-meta">
        <div><dt>模式</dt><dd>{detail.mode}</dd></div>
        <div><dt>调用方式</dt><dd>{detail.invocation_policy}</dd></div>
        <div><dt>版本</dt><dd>{detail.version}</dd></div>
        {detail.required_connectors?.length > 0 && (
          <div><dt>所需连接器</dt><dd>{(detail.required_connectors ?? []).join(", ")}</dd></div>
        )}
      </dl>
      {detail.instructions_summary && (
        <details className="catalog-instructions">
          <summary>指令摘要</summary>
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
  return (
    <div className="catalog-detail-card">
      <div className="catalog-detail-head">
        <h2>{detail.name}</h2>
        <span className={`status-badge ${connectorStateClass(detail.status)}`}>
          {CONNECTOR_STATE_LABEL[detail.status] ?? detail.status}
        </span>
        {!detail.enabled && <span className="status-badge is-warn">已停用</span>}
      </div>
      {detail.description && <p className="catalog-detail-desc">{detail.description}</p>}
      <dl className="catalog-detail-meta">
        <div><dt>类型</dt><dd>{detail.kind}</dd></div>
        <div><dt>传输</dt><dd>{detail.transport_summary}</dd></div>
        <div><dt>认证</dt><dd>{detail.auth_mode}</dd></div>
        <div><dt>命名空间</dt><dd>{detail.tool_filter ?? "—"}</dd></div>
      </dl>

      {detail.auth_mode === "oauth" ? (
        <div className="catalog-auth-row">
          <span className={`status-badge ${authenticated ? "is-success" : "is-warn"}`}>
            {AUTH_STATE_LABEL[authState ?? "not_authenticated"] ?? authState ?? "未授权"}
          </span>
          {!authenticated
            ? <button className="quiet-button" type="button" onClick={onAuthStart} disabled={busy}>授权</button>
            : <button className="quiet-button" type="button" onClick={onLogout} disabled={busy}>取消授权</button>}
          <button className="quiet-button" type="button" onClick={onAuthRefresh} disabled={busy}>刷新状态</button>
          <button className="primary-button" type="button" onClick={onProbe} disabled={busy}>测试连接</button>
        </div>
      ) : (
        <div className="catalog-auth-row">
          <button className="primary-button" type="button" onClick={onProbe} disabled={busy}>测试连接</button>
        </div>
      )}
      {busy && <p className="catalog-detail-hint">正在处理…</p>}

      <details className="catalog-tools" open={(detail.tools?.length ?? 0) > 0}>
        <summary>工具（{detail.tools?.length ?? 0}）</summary>
        {(detail.tools ?? []).length === 0 ? (
          <p className="catalog-detail-hint">尚未探测到工具；运行「测试连接」后可用工具会出现在这里。</p>
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