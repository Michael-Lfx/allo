import { useMemo, useState, type CSSProperties } from "react";
import { useParams } from "react-router";
import { useTranslation } from "react-i18next";
import { Boxes, Plug, Search, Sparkles } from "lucide-react";

import type { Language } from "../i18n";
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

export default function Market() {
  const { lang: raw } = useParams();
  const lang: Language = raw === "en-US" ? "en-US" : "zh-CN";
  const { t } = useTranslation();
  const [tab, setTab] = useState<MarketTab>("experts");
  const [query, setQuery] = useState("");

  const entries = useMemo(() => {
    const list = market[tab];
    const q = query.trim();
    if (!q) return list;
    return list.filter((e) => entryMatches(e, lang, q));
  }, [tab, query, lang]);

  const updated = new Date(market.updatedAt).toLocaleDateString(
    lang === "en-US" ? "en-US" : "zh-CN",
    { year: "numeric", month: "2-digit", day: "2-digit" },
  );

  return (
    <div className="market">
      <div className="market-inner">
        <header className="market-header">
          <p className="eyebrow">{t("landing.eyebrow")}</p>
          <h1>{t("market.title")}</h1>
          <p className="subtle">
            {t("market.subtitle")} {t("market.updated", { date: updated })}
          </p>
        </header>

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
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t("market.searchPlaceholder")}
            aria-label={t("market.searchPlaceholder")}
          />
        </label>

        {entries.length === 0 ? (
          <p className="market-empty">
            {t("market.empty")}
            <span className="subtle">{t("market.emptyHint")}</span>
          </p>
        ) : (
          <ul className="market-grid">
            {entries.map((entry) => {
              const name = entryName(entry, lang);
              const key = "id" in entry ? entry.id : "source" in entry ? entry.source || name : name;
              const tags = entryTags(entry, lang).slice(0, 3);
              return (
                <li className="market-card" key={`${tab}:${key}`}>
                  <span className="market-kind">{t(`market.tabs.${tab}`)}</span>
                  <div className="market-head">
                    {"avatar" in entry && entry.avatar ? (
                      <img
                        className="market-avatar"
                        src={avatarUrl(entry.avatar)}
                        alt=""
                        width={40}
                        height={40}
                        loading="lazy"
                      />
                    ) : (
                      <span
                        className="market-avatar-fallback"
                        aria-hidden="true"
                        style={{ "--avatar-h": String(nameHue(name)) } as CSSProperties}
                      >
                        {name.slice(0, 1).toUpperCase()}
                      </span>
                    )}
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
      </div>
    </div>
  );
}
