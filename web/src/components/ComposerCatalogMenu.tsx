import { useEffect, useMemo, useRef, useState } from "react";
import { Search, Wrench, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useAppStore } from "../store/appStore";
import { pickLocalized, useLocalizedLang } from "../ui/localize";
import { formatError } from "../lib/errors";
import { connectorNeedsAuth, connectorRowStatusLabel } from "./catalog/shared";
import type { AgentSummary, ConnectorSummary, SkillSummary } from "../lib/protocol";

type CatalogKind = "agents" | "skills" | "connectors";
/** Only these two are `@`-mentionable; connectors are switched, not mentioned. */
type PickableCatalogKind = Exclude<CatalogKind, "connectors">;

/**
 * Composer "+" 菜单的目录子菜单：专家(agents) / 技能(skills) / 连接器(connectors)。
 * 数据来自 App Server (`agent/list` `skill/list` `connector/list`)，与 Catalog 页同源。
 * 支持搜索过滤；无数据时显示加载 / 未连接 / 空态。
 *
 * 两类行的行为**不同**，这是刻意的（doc `28`）：
 * - 专家 / 技能：点一行即 `onPick`，Composer 回填 `@name` 并记为结构化 mention；
 * - 连接器：**不是** mention（它换的是宿主的工具面，不是随消息解析的引用）——每行给一个
 *   真正的开关翻宿主的 `enabled`；未授权的 OAuth 连接器另给一颗「连接」发起浏览器流。
 *
 * 行的 DOM 结构是 `div` + **并列**按钮（「连接」与开关）：不能把开关做成整行按钮，
 * 因为行内还有一个按钮，嵌套按钮是非法 HTML。这也与设置里的 MCP 开关行同构——只有开关可点。
 */
export function ComposerCatalogMenu({
  kind,
  onClose,
  onPick,
}: {
  kind: CatalogKind;
  onClose: () => void;
  onPick: (kind: PickableCatalogKind, item: { id: string; name: string }) => void;
}) {
  const { t } = useTranslation();
  const lang = useLocalizedLang();
  const client = useAppStore((s) => s.client);
  const pushToast = useAppStore((s) => s.pushToast);
  const [items, setItems] = useState<Array<AgentSummary | SkillSummary | ConnectorSummary> | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  /** Connector id whose row has a request in flight; blocks a second click. */
  const [busyId, setBusyId] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const ref = useRef<HTMLDivElement | null>(null);

  // Popover-level useClickAway (Composer) owns outside clicks; this submenu
  // stays open when the user clicks the parent menu to switch kind.

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    setItems(null);
    if (!client) {
      setLoading(false);
      return;
    }
    const loader =
      kind === "agents" ? client.agents.list() :
      kind === "skills" ? client.skills.list() :
      client.connectors.list();
    loader
      .then((data) => { if (!cancelled) setItems(data); })
      .catch((err) => { if (!cancelled) setError(err?.message ?? String(err)); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [client, kind]);

  const label =
    kind === "agents" ? t("composer.expert") :
    kind === "skills" ? t("composer.skill") :
    t("composer.connector");

  // Search filter: match name or description (case-insensitive).
  const filtered = useMemo(() => {
    if (!items) return items;
    const q = query.trim().toLowerCase();
    if (!q) return items;
    return items.filter((item) => {
      const name = ("name" in item ? item.name : "").toLowerCase();
      const desc = ("description" in item && item.description ? item.description : "").toLowerCase();
      return name.includes(q) || desc.includes(q);
    });
  }, [items, query]);

  /** Re-read the list so the rows' `status`（以及派生的次行文案）is current. */
  const refreshStatuses = async () => {
    if (!client) return;
    try {
      setItems(await client.connectors.list());
    } catch {
      // Deliberately swallowed: this is a cosmetic refresh **after** a request
      // that already succeeded, and reporting it would attribute an unrelated
      // failure to the action the caller just performed. The switch state came
      // from the action's own response and stays.
    }
  };

  /**
   * Flip one connector's host-level `enabled`.
   *
   * The route is a toggle, so the **response** decides the new state — never the
   * value we asked for. A failure leaves the row untouched and says why:
   * silently showing the switch as moved would claim a capability the host did
   * not grant.
   */
  const toggleConnector = async (connector: ConnectorSummary) => {
    if (!client || busyId) return;
    setBusyId(connector.id);
    try {
      const result = await client.toggleMcpServerEnabled(connector.id);
      setItems((current) =>
        current?.map((item) =>
          item.id === connector.id && "enabled" in item
            ? { ...item, enabled: result.enabled }
            : item,
        ) ?? current,
      );
      await refreshStatuses();
    } catch (caught) {
      pushToast("error", "composer.connectorToggleFailed", { error: formatError(caught) });
    } finally {
      setBusyId(null);
    }
  };

  /**
   * Start the host's OAuth flow for one connector.
   *
   * Same method and semantics as the Catalog drawer's action: the browser flow
   * and the token stay on the trusted host, the client only triggers it. The
   * row's second line keeps whatever the host's most recent probe says —
   * authorizing does not by itself make a connector `connected` (that takes a
   * probe, exactly as in the drawer).
   */
  const connectConnector = async (connector: ConnectorSummary) => {
    if (!client || busyId) return;
    setBusyId(connector.id);
    try {
      const started = await client.connectors.authStart(connector.id);
      if (started.error) {
        pushToast("error", "catalog.authStartFailed", { error: started.error });
        return;
      }
      pushToast("success", "catalog.authStarted");
      await refreshStatuses();
    } catch (caught) {
      pushToast("error", "catalog.authStartFailed", { error: formatError(caught) });
    } finally {
      setBusyId(null);
    }
  };

  return (
    <div className="catalog-submenu" role="menu" aria-label={label} ref={ref}>
      <div className="catalog-submenu-head">
        <div className="catalog-submenu-search">
          <Search aria-hidden="true" size={15} />
          <input
            type="text"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t("composer.catalogSubmenu.search", { label })}
            aria-label={t("composer.catalogSubmenu.search", { label })}
          />
        </div>
        <button type="button" className="catalog-submenu-back" aria-label={t("common.close")} onClick={onClose}>
          <X size={15} />
        </button>
      </div>

      <div className="catalog-submenu-body">
        {loading && <p className="catalog-submenu-note">{t("composer.catalogSubmenu.loading")}</p>}
        {!loading && error && <p className="catalog-submenu-note is-error">{t("composer.catalogSubmenu.error", { error })}</p>}
        {!loading && !error && !client && <p className="catalog-submenu-note">{t("composer.catalogSubmenu.offline")}</p>}
        {!loading && !error && client && filtered?.length === 0 && (
          <p className="catalog-submenu-note">{query.trim() ? t("composer.catalogSubmenu.noResults", { query }) : t("composer.catalogSubmenu.empty", { label })}</p>
        )}
        {!loading && !error && filtered?.map((item) => {
          const name = "name" in item ? item.name : "";
          const displayName =
            "display_name" in item && item.display_name
              ? pickLocalized(item.display_name, lang) || name
              : name;
          const avatar = "avatar_url" in item && item.avatar_url ? item.avatar_url : null;
          const icon = (
            <span className="catalog-submenu-item-icon" aria-hidden="true">
              {avatar ? (
                <img src={avatar.startsWith("http://") || avatar.startsWith("https://") ? avatar : client?.serverRootUrl ? `${client.serverRootUrl}${avatar.startsWith("/") ? "" : "/"}${avatar}` : avatar} alt="" className="catalog-submenu-avatar" />
              ) : (
                <span className="catalog-submenu-letter">{displayName.charAt(0).toUpperCase()}</span>
              )}
            </span>
          );
          if (kind === "connectors") {
            const connector = item as ConnectorSummary;
            const busy = busyId === connector.id;
            return (
              <div className="catalog-submenu-item" key={connector.id}>
                {icon}
                <span className="catalog-submenu-item-text">
                  <span className="catalog-submenu-item-name">{displayName}</span>
                  <span className="catalog-submenu-item-desc">{connectorRowStatusLabel(t, connector)}</span>
                </span>
                {connectorNeedsAuth(connector) && (
                  <button
                    type="button"
                    className="catalog-submenu-connect"
                    disabled={busy}
                    onClick={() => void connectConnector(connector)}
                  >
                    {t("composer.connectorConnect")}
                  </button>
                )}
                <button
                  type="button"
                  role="switch"
                  aria-checked={connector.enabled}
                  aria-label={t("composer.connectorToggleAria", { name: displayName })}
                  className={`catalog-submenu-switch ${connector.enabled ? "is-on" : ""}`}
                  disabled={busy}
                  onClick={() => void toggleConnector(connector)}
                >
                  <span className="catalog-submenu-switch-knob" aria-hidden="true" />
                </button>
              </div>
            );
          }
          const desc =
            "description" in item && item.description ? item.description :
            "model_summary" in item && item.model_summary ? item.model_summary :
            "transport_summary" in item ? item.transport_summary : "";
          return (
            <button
              type="button"
              role="menuitem"
              className="catalog-submenu-item"
              key={(item as { id: string }).id}
              onClick={() => onPick(kind, { id: (item as { id: string }).id, name: displayName })}
            >
              {icon}
              <span className="catalog-submenu-item-text">
                <span className="catalog-submenu-item-name">{displayName}</span>
                {kind !== "agents" && desc && <span className="catalog-submenu-item-desc">{String(desc).slice(0, 60)}</span>}
              </span>
            </button>
          );
        })}
      </div>

      <div className="catalog-submenu-footer">
        <button type="button" className="catalog-submenu-footer-btn" onClick={onClose}>
          <Wrench aria-hidden="true" size={15} />
          <span>{t("composer.catalogSubmenu.manage", { label })}</span>
        </button>
      </div>
    </div>
  );
}
