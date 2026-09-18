/**
 * Marketplace registry panel (W13): add a market, list the registered ones,
 * open one's detail, flip `auto_update`, refresh it, cascade-remove it, and
 * import a single entry from it.
 *
 * **Self-contained on purpose.** It used to be one of the catalog page's four
 * tabs and read half of its state out of that page — the page's error banner,
 * its `store/list` projection for the per-entry install action, its warm-up
 * flag. It now renders in the settings dialog (市场源 分区), a host that cannot
 * show the catalog page's banner, so it owns everything it reads and reports.
 *
 * What it still reads from outside is `store/list`, and only for the entry
 * install *action*'s target (`store/install-entry` addresses a store entry).
 * The installed state itself comes from `market/get`'s own
 * `snapshot.installed_count` projection (`16` D-W13-1 ①) — the same source the
 * cascade dialog reads — so a stale store listing cannot misreport it.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronRight } from "lucide-react";

import { formatError, isRetryableError } from "../../lib/errors";
import { pickEntryTags, pickEntryText, useLocalizedLang } from "../../ui/localize";
import { useAppStore } from "../../store/appStore";
import type {
  MarketplaceDetail,
  MarketplaceSourceKind,
  MarketplaceSummary,
  StoreItem,
} from "../../lib/protocol";
import { DialogShell } from "../dialogs/DialogShell";
import { InitialBadge, MetaRow, marketKindLabel } from "./shared";

export function MarketSourcesPanel() {
  const { t } = useTranslation();
  const lang = useLocalizedLang();
  const client = useAppStore((s) => s.client);
  const pushToast = useAppStore((s) => s.pushToast);

  const [markets, setMarkets] = useState<MarketplaceSummary[] | null>(null);
  const [marketDetail, setMarketDetail] = useState<MarketplaceDetail | null>(null);
  const [marketPath, setMarketPath] = useState("");
  const [marketKind, setMarketKind] = useState<MarketplaceSourceKind>("directory");
  const [marketBusy, setMarketBusy] = useState(false);
  const [marketRefreshBusy, setMarketRefreshBusy] = useState<string | null>(null);
  /** `<marketplace_id>/<entry>` whose entry-level import is in flight (W13). */
  const [marketEntryBusy, setMarketEntryBusy] = useState<string | null>(null);
  /** Cascade-remove confirmation target; null = dialog closed (W13). */
  const [marketRemoveFor, setMarketRemoveFor] = useState<string | null>(null);
  /** Install targets for the entry cards (see the module doc). */
  const [storeItems, setStoreItems] = useState<StoreItem[]>([]);
  const [storeInstallBusy, setStoreInstallBusy] = useState<string | null>(null);
  /** True while the builtin marketplaces are still mirroring (D-SDK-1 ①). */
  const [storePending, setStorePending] = useState(false);
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

  // useEffect必要性：宿主文件与市场注册表都在 React 之外（经 WebSocket 的
  // market/list 与 store/list）；目的：面板挂载时读一次注册表，并为条目安装按钮
  // 取一份 store/list 投影。未采用 ahooks：请求经 useAppStore 的 client，且需要在
  // 卸载后丢弃结果（activeRef），useRequest 的缓存与自动重试都不是这里要的语义；
  // 「挂载时读一次」也无法用 useMemo/事件处理器表达。
  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      if (!client) {
        if (!cancelled) setMarkets(null);
        return;
      }
      try {
        const [list, store] = await Promise.all([client.listMarketplaces(), client.listStore()]);
        if (cancelled) return;
        setMarkets(list);
        setStoreItems(store.items);
        setStorePending(Boolean(store.markets_pending));
      } catch (caught) {
        if (cancelled) return;
        // An unreadable registry is not an empty one: the empty list would
        // render as "还没有市场", which is a different fact.
        setMarkets([]);
        setError(formatError(caught) + (isRetryableError(caught) ? t("common.retryable") : ""));
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, [client, t]);

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
      const result = await client.refreshMarketplace(marketplaceId);
      const [list, detail] = await Promise.all([
        client.listMarketplaces(),
        client.getMarketplace(marketplaceId).catch(() => null),
      ]);
      if (!activeRef.current) return;
      setMarkets(list);
      if (detail) setMarketDetail(detail);
      // W8 余项: a refresh used to be visible only through `last_checked_at`.
      pushToast(
        "success",
        result.changed ? "catalog.marketRefreshDone" : "catalog.marketRefreshUnchanged",
        {
          name: list.find((item) => item.marketplace_id === marketplaceId)?.name ?? marketplaceId,
          count: result.entry_count,
        },
      );
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
    } finally {
      if (activeRef.current) setMarketRefreshBusy(null);
    }
  }, [client, pushToast]);

  const openMarket = useCallback(async (marketplaceId: string) => {
    if (!client) return;
    setError(null);
    try {
      const detail = await client.getMarketplace(marketplaceId);
      if (!activeRef.current) return;
      setMarketDetail(detail);
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
    }
  }, [client]);

  /**
   * R21: open the cascade confirmation on a **fresh** `market/get`. The impact
   * list is the server's pre-removal projection (per-entry snapshots) rather
   * than a re-derivation over the aggregated store listing, so the detail has
   * to be current before the user commits (`16` D-W13-1 ①).
   */
  const openRemoveDialog = useCallback(async (marketplaceId: string) => {
    if (client) {
      try {
        const detail = await client.getMarketplace(marketplaceId);
        if (!activeRef.current) return;
        setMarketDetail(detail);
      } catch {
        // A failed refresh must not block the dialog — the user can still
        // cancel, and the list falls back to the detail already on screen.
      }
    }
    if (activeRef.current) setMarketRemoveFor(marketplaceId);
  }, [client]);

  /**
   * W13: cascade removal is confirmed through a dialog that lists the entries it
   * will uninstall (`marketRemoveFor`) instead of a bare `window.confirm`.
   */
  const confirmRemoveMarket = useCallback(async () => {
    const marketplaceId = marketRemoveFor;
    if (!client || !marketplaceId) return;
    setMarketBusy(true);
    setError(null);
    try {
      await client.removeMarketplace(marketplaceId, true);
      const list = await client.listMarketplaces();
      if (!activeRef.current) return;
      setMarkets(list);
      setMarketDetail(null);
      setMarketRemoveFor(null);
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
    } finally {
      if (activeRef.current) setMarketBusy(false);
    }
  }, [client, marketRemoveFor]);

  /** W13: flip `auto_update`, then read `market/list` back (no optimistic guess). */
  const toggleMarketAutoUpdate = useCallback(async (marketplaceId: string, enabled: boolean) => {
    if (!client) return;
    setMarketBusy(true);
    setError(null);
    try {
      const updated = await client.setMarketplaceAutoUpdate(marketplaceId, enabled);
      const list = await client.listMarketplaces();
      if (!activeRef.current) return;
      setMarkets(list);
      setMarketDetail((current) =>
        current && current.marketplace_id === marketplaceId
          ? { ...current, auto_update: updated.auto_update, enabled: updated.enabled }
          : current,
      );
      pushToast("success", updated.auto_update ? "catalog.marketAutoUpdateOnToast" : "catalog.marketAutoUpdateOffToast");
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
    } finally {
      if (activeRef.current) setMarketBusy(false);
    }
  }, [client, pushToast]);

  /**
   * W13: entry-level import. Deliberately distinct from `store/install-entry`:
   * this only imports the provenance-linked snapshot and leaves the install
   * state untouched.
   */
  const importMarketEntry = useCallback(async (marketplaceId: string, entryName: string) => {
    if (!client) return;
    setMarketEntryBusy(`${marketplaceId}/${entryName}`);
    setError(null);
    try {
      const result = await client.importMarketplaceEntry(marketplaceId, entryName);
      const detail = await client.getMarketplace(marketplaceId).catch(() => null);
      if (!activeRef.current) return;
      if (detail) setMarketDetail(detail);
      pushToast("success", result.reused ? "catalog.marketEntryImportReused" : "catalog.marketEntryImportDone");
    } catch (caught) {
      if (!activeRef.current) return;
      reportError(caught);
    } finally {
      if (activeRef.current) setMarketEntryBusy(null);
    }
  }, [client, pushToast]);

  /** `store/install-entry` for one entry, then re-read both projections. */
  const runEntryInstall = useCallback(async (item: StoreItem) => {
    if (!client) return;
    setStoreInstallBusy(item.id);
    setError(null);
    try {
      const result = await client.installStoreEntry(item.marketplace_id, item.entry_name);
      const [list, store] = await Promise.all([client.listMarketplaces(), client.listStore()]);
      const detail = await client.getMarketplace(item.marketplace_id).catch(() => null);
      if (!activeRef.current) return;
      setMarkets(list);
      setStoreItems(store.items);
      if (detail) setMarketDetail(detail);
      // W8 余项: the install used to end silently.
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
  }, [client, pushToast]);

  return (
    <div className="market-imports market-sources-panel">
      {error && (
        <div className="market-alert" role="alert">
          <span>{error}</span>
          <button type="button" aria-label={t("common.closeError")} onClick={() => setError(null)}>✕</button>
        </div>
      )}

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
              <option value="zip">{t("catalog.marketKindZip")}</option>
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
                    : marketKind === "zip"
                      ? t("catalog.marketZipPlaceholder")
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
          <p className="market-empty">
            {storePending ? t("catalog.storePending") : t("catalog.noMarkets")}
          </p>
        )}
        {/* Partial catalog during the background warm-up (D-SDK-1 ①): the
            list is non-empty but the builtin markets may still be arriving. */}
        {storePending && markets !== null && markets.length > 0 && (
          <p className="market-pending-note">{t("catalog.storePending")}</p>
        )}
        {markets !== null && markets.length > 0 && (
          <div className="market-list">
            {markets.map((market) => (
              <button className={`market-card${marketDetail?.marketplace_id === market.marketplace_id ? " is-active" : ""}`} type="button" key={market.marketplace_id}
                onClick={() => void openMarket(market.marketplace_id)}>
                <div className="market-card-top">
                  <InitialBadge name={market.name} />
                  <div className="market-card-main">
                    <span className="market-card-title">{market.name}</span>
                    <span className="market-card-sub">
                      {marketKindLabel(t, market.source_kind)} · {t("catalog.marketEntries", { count: market.entry_count })}
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
                {/* The version is optional on the wire, so it is dropped rather
                    than rendered as `v?`. */}
                <span className="market-card-sub">
                  {[marketKindLabel(t, marketDetail.source_kind), marketDetail.version ? `v${marketDetail.version}` : null]
                    .filter(Boolean)
                    .join(" · ")}
                </span>
              </div>
              <div className="market-market-detail-actions">
                {/* A switch, not a button whose label is its own state: the MCP
                    分区的 per-server switch is the same control, so both read
                    the same way. The state text stays as the tooltip. */}
                <span className="market-switch-row">
                  <span>{t("catalog.marketAutoUpdate")}</span>
                  <button
                    className={`switch-pill${marketDetail.auto_update ? " is-on" : ""}`}
                    type="button"
                    role="switch"
                    aria-checked={marketDetail.auto_update}
                    aria-label={t("catalog.marketAutoUpdate")}
                    title={marketDetail.auto_update
                      ? t("catalog.marketAutoUpdateToggleOn")
                      : t("catalog.marketAutoUpdateToggleOff")}
                    disabled={marketBusy}
                    onClick={() => void toggleMarketAutoUpdate(marketDetail.marketplace_id, !marketDetail.auto_update)}
                  >
                    <span className="switch-pill-knob" aria-hidden="true" />
                  </button>
                </span>
                <button className="quiet-button" type="button"
                  disabled={marketRefreshBusy === marketDetail.marketplace_id}
                  onClick={() => void refreshMarket(marketDetail.marketplace_id)}>
                  {marketRefreshBusy === marketDetail.marketplace_id
                    ? t("catalog.marketRefreshing")
                    : t("catalog.marketRefresh")}
                </button>
                {/* Outline, not a solid red block: removal is confirmed in a
                    dialog anyway, and this row's other controls are quiet. */}
                <button className="quiet-button is-destructive" type="button" disabled={marketBusy}
                  onClick={() => void openRemoveDialog(marketDetail.marketplace_id)}>
                  {t("catalog.marketRemove")}
                </button>
              </div>
            </div>
            {/* W13: only the registry fields the wire actually carries —
                `revision` / last-checked are absent from the summary type
                (deviation D-W13-1). */}
            <dl className="market-meta">
              <MetaRow label={t("catalog.marketIdLabel")} value={marketDetail.marketplace_id} mono />
              <MetaRow
                label={t("catalog.marketEnabled")}
                value={marketDetail.enabled ? t("catalog.marketEnabledOn") : t("catalog.marketEnabledOff")}
              />
              <MetaRow label={t("catalog.marketEntriesLabel")} value={String(marketDetail.entry_count)} />
              <MetaRow label={t("catalog.marketAddedAt")} value={new Date(marketDetail.added_at).toLocaleString()} />
              <MetaRow label={t("catalog.marketRevision")} value={marketDetail.resolved_revision} mono />
              {/* `last_checked_at` is written by refresh only, so a freshly
                  added market legitimately has none — show it explicitly
                  rather than dropping the row silently. */}
              <MetaRow
                label={t("catalog.marketLastChecked")}
                value={marketDetail.last_checked_at ? new Date(marketDetail.last_checked_at).toLocaleString() : "—"}
              />
            </dl>
            <div className="market-list">
              {marketDetail.entries.map((entry) => {
                const storeItem = storeItems.find(
                  (item) => item.marketplace_id === marketDetail.marketplace_id && item.entry_name === entry.name,
                );
                const busy = storeInstallBusy === `${marketDetail.marketplace_id}/${entry.name}`;
                const entryBusy = marketEntryBusy === `${marketDetail.marketplace_id}/${entry.name}`;
                // Install state comes from the server-projected entry snapshot
                // on `market/get` — the same source the cascade dialog reads —
                // instead of the aggregated store listing (`16` D-W13-1 ①).
                // `storeItem` is still needed for the install *action*, which
                // addresses the store entry.
                const installed = (entry.snapshot?.installed_count ?? 0) > 0;
                // R28 / D8=A: the manifest's `name_{lang}` / `description_{lang}`
                // variants fall back to the baseline fields, resolved by the UI
                // language.
                const entryName = pickEntryText(entry.localized, "name", lang, entry.name) ?? entry.name;
                const entryTags = pickEntryTags(entry.localized, lang) ?? entry.keywords ?? [];
                // `02` §11.1: the entry is listed so the reason is readable, but
                // neither action can succeed.
                const blocked = entry.blocked_reason ?? null;
                return (
                  <div className="market-card market-entry-card" key={`${marketDetail.marketplace_id}/${entry.name}`}>
                    <div className="market-card-top">
                      <InitialBadge name={entryName} />
                      <div className="market-card-main">
                        <span className="market-card-title">{entryName}</span>
                        <span className="market-card-sub">
                          {pickEntryText(entry.localized, "description", lang, entry.description) ??
                            entry.source}
                        </span>
                        {entryTags.length > 0 && (
                          <span className="market-tags">
                            {entryTags.slice(0, 3).map((tag) => (
                              <span className="market-tag" key={tag}>
                                {tag}
                              </span>
                            ))}
                          </span>
                        )}
                      </div>
                      {blocked && (
                        <span className="market-tag is-blocked" title={blocked}>
                          {t("catalog.storeBlocked")}
                        </span>
                      )}
                      {installed && <span className="market-tag is-status is-success">{t("catalog.storeInstalled")}</span>}
                    </div>
                    {/* The actions get their own line. This card is ~315px wide
                        inside the settings dialog and both buttons are
                        `flex-shrink: 0`, which used to squeeze the text column
                        down to about two characters — entry names rendered as
                        「腾讯…」 and descriptions wrapped every second glyph. */}
                    <div className="market-entry-actions">
                      {!installed && (
                        <button
                          className="primary-button market-entry-import"
                          type="button"
                          disabled={busy || !storeItem || Boolean(blocked)}
                          title={blocked ?? undefined}
                          onClick={() => { if (storeItem) void runEntryInstall(storeItem); }}
                        >
                          {busy ? t("catalog.storeInstalling") : t("catalog.storeInstall")}
                        </button>
                      )}
                      {/* W13: import-only — a provenance-linked snapshot without
                          touching install state (`store/install-entry` above is
                          the install path). */}
                      <button
                        className="quiet-button market-entry-import"
                        type="button"
                        disabled={entryBusy || Boolean(blocked)}
                        title={blocked ?? undefined}
                        onClick={() => void importMarketEntry(marketDetail.marketplace_id, entry.name)}
                      >
                        {entryBusy ? t("catalog.marketEntryImporting") : t("catalog.marketEntryImport")}
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        )}

        {marketRemoveFor && (
          <DialogShell
            onClose={() => setMarketRemoveFor(null)}
            labelledBy="market-remove-title"
            titleId="market-remove-title"
            title={t("catalog.marketRemoveTitle")}
          >
            <p className="dialog-intro">
              {t("catalog.marketRemoveBody", { name: marketDetail?.name ?? marketRemoveFor })}
            </p>
            {(() => {
              // Cascade target list is the server's pre-removal projection —
              // `market/get` entries carry `snapshot.installed_count`,
              // refreshed by `openRemoveDialog` — instead of a re-derivation
              // over the aggregated store listing (R21).
              const affected = (marketDetail?.entries ?? []).filter(
                (entry) => (entry.snapshot?.installed_count ?? 0) > 0,
              );
              if (affected.length === 0) {
                return <p className="dialog-intro">{t("catalog.marketRemoveNone")}</p>;
              }
              return (
                <ul className="market-remove-list">
                  <li className="market-remove-list-title">{t("catalog.marketRemoveSnapshotsLabel")}</li>
                  {affected.map((entry) => (
                    <li key={entry.name}>{entry.name}</li>
                  ))}
                </ul>
              );
            })()}
            <div className="dialog-actions">
              <button className="quiet-button" type="button" onClick={() => setMarketRemoveFor(null)}>
                {t("common.cancel")}
              </button>
              <button className="danger-button" type="button" disabled={marketBusy} onClick={() => void confirmRemoveMarket()}>
                {marketBusy ? t("catalog.marketRemoving") : t("catalog.marketRemove")}
              </button>
            </div>
          </DialogShell>
        )}
      </div>
    </div>
  );
}
