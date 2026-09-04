import { Link, useParams } from "react-router";
import { useTranslation } from "react-i18next";
import { ArrowUpRight } from "lucide-react";
import type { CSSProperties } from "react";

import type { Language } from "../i18n";
import { marketCounts, marketTotal } from "../lib/market";

/** Real market snapshot: 3 expert cards + live counts → /market. */
export default function MarketStrip() {
  const { t } = useTranslation();
  const { lang: raw } = useParams();
  const lang: Language = raw === "en-US" ? "en-US" : "zh-CN";

  return (
    <section className="market-strip" id="market-strip">
      <div className="market-strip-inner">
        <p className="eyebrow eyebrow-center">
          <span className="eyebrow-index">03</span>
          {t("landing.eyebrow")}
        </p>
        <h2 data-reveal>{t("landing.marketStrip.title")}</h2>
        <p className="subtle subtle-center" data-reveal>
          {t("landing.marketStrip.subtitle")}
        </p>

        <div className="ms-stats" data-reveal>
          <div className="ms-stat">
            <span className="ms-stat-value" data-count={String(marketTotal)}>
              {marketTotal}
            </span>
            <span className="ms-stat-label">{t("landing.marketStat")}</span>
          </div>
          <div className="ms-stat">
            <span className="ms-stat-value" data-count={String(marketCounts.experts)}>
              {marketCounts.experts}
            </span>
            <span className="ms-stat-label">{t("market.tabs.experts")}</span>
          </div>
          <div className="ms-stat">
            <span className="ms-stat-value" data-count={String(marketCounts.skills)}>
              {marketCounts.skills}
            </span>
            <span className="ms-stat-label">{t("market.tabs.skills")}</span>
          </div>
          <div className="ms-stat">
            <span className="ms-stat-value" data-count={String(marketCounts.connectors)}>
              {marketCounts.connectors}
            </span>
            <span className="ms-stat-label">{t("market.tabs.connectors")}</span>
          </div>
        </div>

        <Link className="btn btn-invert btn-lg ms-cta" to={`/${lang}/market`} data-reveal>
          {t("landing.marketStrip.cta")}
          <ArrowUpRight size={18} aria-hidden="true" />
        </Link>
      </div>
    </section>
  );
}
