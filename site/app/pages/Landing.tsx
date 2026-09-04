import { Link, useParams } from "react-router";
import { useTranslation } from "react-i18next";
import { Boxes, GitBranch, HardDrive, Terminal } from "lucide-react";

import type { Language } from "../i18n";
import { marketCounts, marketTotal } from "../lib/market";
import { useCountUp, useRevealAll, useSpotlight, revealDelay } from "../lib/effects";
import WorkbenchShowcase from "../components/WorkbenchShowcase";
import MarketStrip from "../components/MarketStrip";
import DevSection from "../components/DevSection";
import FaqSection from "../components/FaqSection";
import WorkflowSteps from "../components/WorkflowSteps";
import DownloadCTA from "../components/DownloadCTA";
import CopyButton from "../components/CopyButton";

const FEATURE_ICONS = { workbench: Boxes, observability: GitBranch, localFirst: HardDrive, oneCmd: Terminal };
const FEATURE_KEYS = Object.keys(FEATURE_ICONS) as (keyof typeof FEATURE_ICONS)[];

const QUICK_CMD = "flowy-agent-store";
const PLATFORM_KEYS = ["macos", "windows", "linux"] as const;

export default function Landing() {
  const { lang: raw } = useParams();
  const lang: Language = raw === "en-US" ? "en-US" : "zh-CN";
  const { t } = useTranslation();
  const platforms = t("landing.platforms", { returnObjects: true }) as Record<string, string>;

  // Decorative page-level effects (reveal on scroll, spotlight, counters).
  useRevealAll();
  useSpotlight(".feature-card");
  useCountUp();

  return (
    <>
      <section className="hero">
        <div className="hero-aurora" aria-hidden="true">
          <i className="aurora-blob aurora-a" />
          <i className="aurora-blob aurora-b" />
          <i className="aurora-blob aurora-c" />
        </div>
        <div className="hero-grid" aria-hidden="true" />
        <div className="hero-glow" aria-hidden="true" />
        <div className="hero-inner">
          <p className="eyebrow" style={revealDelay(0)}>{t("landing.eyebrow")}</p>
          <h1 className="hero-title" style={revealDelay(70)}>{t("landing.heroTitle")}</h1>
          <p className="hero-sub" style={revealDelay(150)}>{t("landing.heroSubtitle")}</p>
          <div className="hero-actions" style={revealDelay(230)}>
            <a className="btn btn-primary btn-lg" href="#download">
              {t("landing.heroCtaDownload")}
            </a>
            <Link className="btn btn-quiet btn-lg" to={`/${lang}/market`}>
              {t("landing.heroCtaMarket")}
              <span className="ms-chip">{marketTotal}</span>
            </Link>
            <Link className="btn btn-quiet btn-lg" to={`/${lang}/docs`}>
              {t("landing.heroCtaDocs")}
            </Link>
          </div>

          <figure className="hero-terminal" style={revealDelay(330)}>
            <span className="term-beam" aria-hidden="true" />
            <figcaption className="term-head">
              <span className="term-dots" aria-hidden="true">
                <i />
                <i />
                <i />
              </span>
              <span className="term-title">terminal</span>
              <span className="term-copy">
                <CopyButton text={QUICK_CMD} label={t("landing.download.copy")} tone="dark" />
              </span>
            </figcaption>
            <div className="term-body">
              <p className="term-line">
                <span className="term-prompt" aria-hidden="true">
                  $
                </span>
                <code>{QUICK_CMD}</code>
                <span className="term-caret" aria-hidden="true" />
              </p>
              <p className="term-line term-ok term-line-2">
                <span aria-hidden="true">✓</span> {t("landing.heroTerminalListening")}
              </p>
              <p className="term-line term-ok term-line-3">
                <span aria-hidden="true">✓</span> {t("landing.heroTerminalOpened")}
              </p>
            </div>
          </figure>

          <ul className="hero-pills" aria-label="platforms" style={revealDelay(430)}>
            {PLATFORM_KEYS.map((os) => (
              <li className="pill" key={os}>
                {platforms[os]}
              </li>
            ))}
          </ul>
        </div>
      </section>

      <WorkbenchShowcase />

      <section className="features" id="features">
        <div className="section-inner">
          <p className="eyebrow" data-reveal>
            <span className="eyebrow-index">01</span>
            {t("landing.eyebrow")}
          </p>
          <h2 data-reveal>{t("landing.featureTitle")}</h2>
          <p className="subtle" data-reveal>{t("landing.featureSubtitle")}</p>
          <div className="feature-grid">
            {FEATURE_KEYS.map((key, i) => {
              const Icon = FEATURE_ICONS[key];
              return (
                <article className="feature-card" data-reveal style={revealDelay(i * 90)} key={key}>
                  <span className="feature-icon" aria-hidden="true">
                    <Icon size={20} />
                  </span>
                  <h3>{t(`landing.features.${key}.title`)}</h3>
                  <p>{t(`landing.features.${key}.desc`)}</p>
                </article>
              );
            })}
          </div>
        </div>
      </section>

      <WorkflowSteps />
      <MarketStrip />
      <DevSection />
      <FaqSection />
      <DownloadCTA variant="full" lang={lang} />
    </>
  );
}
