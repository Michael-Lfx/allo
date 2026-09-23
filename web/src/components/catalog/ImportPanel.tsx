/**
 * Import panel: run an import from a local source path, list the import
 * history, open one snapshot's detail (components + compatibility), and drive
 * that snapshot's install state (install / enable / disable / uninstall).
 *
 * **Self-contained on purpose**, for the same reason as `MarketSourcesPanel`:
 * it used to be one of the catalog page's four tabs and borrowed the page's
 * error banner and its `imports` list. It now opens as a dialog from the
 * 技能 / 连接器 tabs, where none of the page's state is in scope, so it owns
 * both — and it re-reads `import/list` after every write rather than assuming
 * the page will.
 *
 * Two filters the tab version applied are deliberately gone: the search box it
 * filtered on was never rendered while this surface was open (the query input
 * only mounts on the store / installed tabs), and the `category` it filtered
 * by had no control that could set it. A hidden filter over a visible list is
 * a lie about what the user is looking at.
 *
 * The detail overlay is rendered here rather than by the host page: it is
 * `position: fixed`, so it covers the dialog that hosts this panel and closing
 * it returns to the list.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { CircleAlert, X } from "lucide-react";

import { formatError, isRetryableError } from "../../lib/errors";
import { useAppStore } from "../../store/appStore";
import type {
  ImportDetail,
  ImportResult,
  ImportSourceKind,
  ImportSummary,
  InstallResult,
  InstallState,
  InstallStatus,
} from "../../lib/protocol";
import {
  CompatChips,
  InitialBadge,
  MetaList,
  MetaRow,
  Tags,
  importKindLabel,
  importStatusClass,
  importStatusLabel,
  installStateClass,
  installStateLabel,
  type InstallToggleKind,
} from "./shared";

export function ImportPanel({ initialKind = "codebuddy-plugin" }: { initialKind?: ImportSourceKind }) {
  const { t } = useTranslation();
  const client = useAppStore((s) => s.client);

  const [imports, setImports] = useState<ImportSummary[] | null>(null);
  const [importResult, setImportResult] = useState<ImportResult | null>(null);
  const [importPath, setImportPath] = useState("");
  const [importKind, setImportKind] = useState<ImportSourceKind>(initialKind);
  const [importBusy, setImportBusy] = useState(false);
  const [importDetail, setImportDetail] = useState<ImportDetail | null>(null);
  const [installStatus, setInstallStatus] = useState<InstallStatus | null>(null);
  const [installResult, setInstallResult] = useState<InstallResult | null>(null);
  const [detailBusy, setDetailBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

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

  // useEffect必要性：导入历史在 React 之外（经 WebSocket 的 import/list）；目的：
  // 面板打开时读一次历史。未采用 ahooks：请求经 useAppStore 的 client，且要在卸载后
  // 丢弃结果（activeRef），useRequest 的缓存/自动重试不是这里要的语义；「打开时读一次」
  // 也无法用 useMemo/事件处理器表达。
  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      if (!client) {
        if (!cancelled) setImports(null);
        return;
      }
      try {
        const list = await client.listImports();
        if (!cancelled) setImports(list);
      } catch (caught) {
        if (cancelled) return;
        // An unreadable history is not an empty one.
        setImports([]);
        setError(formatError(caught) + (isRetryableError(caught) ? t("common.retryable") : ""));
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, [client, t]);

  const closeDetail = useCallback(() => {
    setImportDetail(null);
    setInstallStatus(null);
    setInstallResult(null);
    setDetailBusy(null);
  }, []);

  const openImport = useCallback(async (snapshotId: string) => {
    if (!client) return;
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

  const toggleInstall = useCallback(async (
    kind: InstallToggleKind,
    snapshotId: string,
    componentId: string,
  ) => {
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

  return (
    <div className="market-imports">
      {error && (
        <div className="market-alert" role="alert">
          <CircleAlert size={15} strokeWidth={1.8} />
          <span>{error}</span>
          <button type="button" aria-label={t("common.closeError")} onClick={() => setError(null)}>✕</button>
        </div>
      )}

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
                {importKindLabel(t, importResult.source_kind)} · {t("catalog.importComponents", { count: importResult.component_count })}
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
        {client && imports !== null && imports.length === 0 && (
          <p className="market-empty">{t("catalog.noImports")}</p>
        )}
        {(imports ?? []).map((item) => (
          <ImportHistoryCard key={item.snapshot_id} item={item} onOpen={() => void openImport(item.snapshot_id)} />
        ))}
      </div>

      {importDetail && (
        <div className="market-drawer-mask" onClick={closeDetail} role="presentation">
          <aside className="market-drawer" role="dialog" aria-modal="true" onClick={(event) => event.stopPropagation()}>
            <button className="icon-button market-drawer-close" type="button" aria-label={t("common.close")} onClick={closeDetail}>
              <X size={18} strokeWidth={1.7} />
            </button>
            <ImportDrawer
              detail={importDetail}
              installStatus={installStatus}
              installResult={installResult}
              busy={detailBusy}
              onInstall={() => void runInstall(importDetail.snapshot_id)}
              onToggle={(kind, componentId) => void toggleInstall(kind, importDetail.snapshot_id, componentId)}
            />
          </aside>
        </div>
      )}
    </div>
  );
}

/**
 * One import-history card, prop-driven so a server render can assert exactly
 * what the dialog shows (`ImportPanel` itself is the wired half — its history
 * loads in an effect, which `renderToStaticMarkup` never runs).
 */
export function ImportHistoryCard({
  item,
  onOpen,
}: {
  item: ImportSummary;
  onOpen: () => void;
}) {
  const { t } = useTranslation();
  return (
    <button className="market-card market-import-card" type="button" onClick={onOpen}>
      <div className="market-card-top">
        <InitialBadge name={item.name} />
        <div className="market-card-main">
          <span className="market-card-title">{item.name} <small>v{item.version}</small></span>
          <span className="market-card-sub">
            {importKindLabel(t, item.source_kind)} · {t("catalog.importComponents", { count: item.component_count })}
          </span>
          <span className="market-import-when">{new Date(item.imported_at).toLocaleString()}</span>
        </div>
        <span className={`status-dot ${importStatusClass(item.status)}`} aria-hidden="true" />
      </div>
      <span className={`market-tag is-status ${importStatusClass(item.status)}`}>{importStatusLabel(t, item.status)}</span>
    </button>
  );
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
            <span className="market-tag">{importKindLabel(t, detail.source_kind)}</span>
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
