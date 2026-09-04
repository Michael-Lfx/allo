import { useEffect, useMemo, useRef, useState } from "react";
import { AtSign, Bot, Plug, Search, Sparkles, Wrench, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useAppStore } from "../store/appStore";
import type { AgentSummary, ConnectorSummary, SkillSummary } from "../lib/protocol";

type CatalogKind = "agents" | "skills" | "connectors";

/**
 * Composer "+" 菜单的目录子菜单：专家(agents) / 技能(skills) / 连接器(connectors)。
 * 数据来自 App Server (`agent/list` `skill/list` `connector/list`)，与 Catalog 页同源。
 * 支持搜索过滤；无数据时显示加载 / 未连接 / 空态。
 *
 * `onPick` 选中目录项时触发；Composer 回填 `@name` 并记为结构化 mention。
 */
export function ComposerCatalogMenu({
  kind,
  onClose,
  onPick,
}: {
  kind: CatalogKind;
  onClose: () => void;
  onPick: (kind: CatalogKind, item: { id: string; name: string }) => void;
}) {
  const { t } = useTranslation();
  const client = useAppStore((s) => s.client);
  const [items, setItems] = useState<Array<AgentSummary | SkillSummary | ConnectorSummary> | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
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
              ? (item.display_name.zh || item.display_name.en || name)
              : name;
          const desc =
            "description" in item && item.description ? item.description :
            "model_summary" in item && item.model_summary ? item.model_summary :
            "transport_summary" in item ? item.transport_summary : "";
          const avatar = "avatar_url" in item && item.avatar_url ? item.avatar_url : null;
          const enabled = "enabled" in item ? item.enabled : "status" in item && item.status ? (item.status === "connected" || item.status === "installed") : true;
          const status = "status" in item ? item.status : "";
          return (
            <button
              type="button"
              role="menuitem"
              className="catalog-submenu-item"
              key={(item as { id: string }).id}
              onClick={() => onPick(kind, { id: (item as { id: string }).id, name: displayName })}
            >
              <span className="catalog-submenu-item-icon" aria-hidden="true">
                {avatar ? (
                  <img src={avatar.startsWith("http://") || avatar.startsWith("https://") ? avatar : client?.serverRootUrl ? `${client.serverRootUrl}${avatar.startsWith("/") ? "" : "/"}${avatar}` : avatar} alt="" className="catalog-submenu-avatar" />
                ) : (
                  <span className="catalog-submenu-letter">{displayName.charAt(0).toUpperCase()}</span>
                )}
              </span>
              <span className="catalog-submenu-item-text">
                <span className="catalog-submenu-item-name">{displayName}</span>
                {kind !== "agents" && desc && <span className="catalog-submenu-item-desc">{String(desc).slice(0, 60)}</span>}
              </span>
              {kind === "connectors" && (
                <span className={`catalog-submenu-switch ${enabled ? "is-on" : ""}`} aria-hidden="true"><span className="catalog-submenu-switch-knob" /></span>
              )}
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
