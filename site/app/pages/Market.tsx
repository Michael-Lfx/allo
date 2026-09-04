import { useMemo, useRef, useState, type CSSProperties, type ReactEventHandler } from "react";
import { useParams } from "react-router";
import { useTranslation } from "react-i18next";
import { Boxes, Plug, Search, Sparkles, X } from "lucide-react";

import type { Language } from "../i18n";
import { useSpotlight } from "../lib/effects";
import {
  MARKET_TABS,
  avatarUrl,
  entryDescription,
  entryMatches,
  entryName,
  entryTags,
  market,
  marketCounts,
  nameHue,
  type MarketTab,
} from "../lib/market";

const TAB_ICONS: Record<MarketTab, typeof Boxes> = {
  experts: Sparkles,
  skills: Boxes,
  connectors: Plug,
};

/** Avatar with fade-in on load and a hue fallback when the URL 404s. */
function EntryAvatar({ src, name }: { src: string | null; name: string }) {
  const [failed, setFailed] = useState(false);
  const onLoad: ReactEventHandler<HTMLImageElement> = (e) =>
    e.currentTarget.classList.add("is-loaded");
  if (!src || failed) {
    return (
      <span
        className="market-avatar-fallback"
        aria-hidden="true"
        style={{ "--avatar-h": String(nameHue(name)) } as CSSProperties}
      >
        {name.slice(0, 1).toUpperCase()}
      </span>
    );
  }
  return (
    <img
      className="market-avatar"
      src={avatarUrl(src)}
      alt=""
      width={40}
      height={40}
      loading="lazy"
      onLoad={onLoad}
      onError={() => setFailed(true)}
    />
  );
}

export default function Market() {
  const { lang: raw } = useParams();
  const lang: Language = raw === "en-US" ? "en-US" : "zh-CN";
  const { t } = useTranslation();
  const [tab, setTab] = useState<MarketTab>("experts");
  const [query, setQuery] = useState("");
  const searchRef = useRef<HTMLInputElement>(null);
  useSpotlight(".market-card");

  const entries = useMemo(() => {
    const list = market[tab];
    const q = query.trim();
    if (!q) return list;
    return list.filter((e) => entryMatches(e, lang, q));
  }, [tab, query, lang]);

  // Re-trigger the card entrance animation whenever the result set changes.
  const gridKey = `${tab}:${query.trim()}`;

  const updated = new Date(market.updatedAt).toLocaleDateString(
    lang === "en-US" ? "en-US" : "zh-CN",
    { year: "numeric", month: "2-digit", day: "2-digit" },
  );

  return (
    <div className="market">
      <div className="market-inner">
        <header className="market-header">
          <p className="eyebrow">{t("landing.eyebrow")}</p>
          <h1 className="page-title-gradient">{t("market.title")}</h1>
          <p className="subtle">{t("market.subtitle")}</p>
          <p className="market-updated">{t("market.updated", { date: updated })}</p>
        </header>

        <div className="market-toolbar">
          <div className="market-tabs" role="tablist" aria-label={t("market.title")}>
            {MARKET_TABS.map((key) => {
              const Icon = TAB_ICONS[key];
              return (
                <button
                  key={key}
                  role="tab"
                  aria-selected={tab === key}
                  className={tab === key ? "market-tab is-active" : "market-tab"}
                  onClick={() => setTab(key)}
                >
                  <Icon size={16} aria-hidden="true" />
                  {t(`market.tabs.${key}`)}
                  <span className="market-tab-count">{marketCounts[key]}</span>
                </button>
              );
            })}
          </div>
          <label className="market-search">
            <Search size={16} aria-hidden="true" />
            <input
              ref={searchRef}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") setQuery("");
              }}
              placeholder={t("market.searchPlaceholder")}
              aria-label={t("market.searchPlaceholder")}
            />
            {query ? (
              <button
                type="button"
                className="market-search-clear"
                onClick={() => {
                  setQuery("");
                  searchRef.current?.focus();
                }}
                aria-label="Clear search"
              >
                <X size={14} />
              </button>
            ) : (
              <kbd className="market-kbd">/</kbd>
            )}
          </label>
        </div>

        {entries.length === 0 ? (
          <div className="market-empty">
            <Search size={28} aria-hidden="true" />
            <p>{t("market.empty")}</p>
            <span className="subtle">{t("market.emptyHint")}</span>
          </div>
        ) : (
          <ul className="market-grid" key={gridKey}>
            {entries.map((entry, i) => {
              const name = entryName(entry, lang);
              const key = "id" in entry ? entry.id : "source" in entry ? entry.source || name : name;
              const tags = entryTags(entry, lang).slice(0, 3);
              return (
                <li
                  className="market-card"
                  key={`${tab}:${key}`}
                  style={{ "--card-i": i } as CSSProperties}
                >
                  <div className="market-head">
                    <EntryAvatar src={"avatar" in entry ? entry.avatar : null} name={name} />
                    <h3>
                      {name}
                      {"version" in entry && entry.version ? (
                        <span className="market-version">v{entry.version}</span>
                      ) : null}
                    </h3>
                  </div>
                  <p>{entryDescription(entry, lang)}</p>
                  {tags.length > 0 ? (
                    <div className="market-tags">
                      {tags.map((tag) => (
                        <span className="market-tag" key={tag}>
                          {tag}
                        </span>
                      ))}
                    </div>
                  ) : null}
                </li>
              );
            })}
          </ul>
        )}
        <p className="market-count">
          {entries.length} / {marketCounts[tab]} · {t(`market.tabs.${tab}`)}
        </p>
      </div>
    </div>
  );
}
