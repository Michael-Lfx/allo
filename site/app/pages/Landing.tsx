import { Link, useParams } from "react-router";
import { useTranslation } from "react-i18next";
import { Box, HardDrive, Layers, Terminal } from "lucide-react";

import type { Language } from "../i18n";
import { marketTotal } from "../lib/market";
import WorkflowSteps from "../components/WorkflowSteps";
import DownloadCTA from "../components/DownloadCTA";
import CopyButton from "../components/CopyButton";

const FEATURE_ICONS = { singleBinary: Box, localFirst: HardDrive, catalog: Layers, cliUi: Terminal };

const QUICK_CMD = "flowy-agent-store";
const PLATFORM_KEYS = ["macos", "windows", "linux"] as const;

export default function Landing() {
  const { lang: raw } = useParams();
  const lang: Language = raw === "en-US" ? "en-US" : "zh-CN";
  const { t } = useTranslation();
  const features = t("landing.features", { returnObjects: true }) as Record<
    string,
    { title: string; desc: string }
  >;
  const platforms = t("landing.platforms", { returnObjects: true }) as Record<string, string>;

  return (
    <>
      <section className="hero">
        <div className="hero-inner">
          <p className="eyebrow">{t("landing.eyebrow")}</p>
          <h1>{t("landing.heroTitle")}</h1>
          <p className="hero-sub">{t("landing.heroSubtitle")}</p>
          <div className="hero-actions">
            <a className="btn btn-primary btn-lg" href="#download">
              {t("landing.heroCtaDownload")}
            </a>
            <Link className="btn btn-quiet btn-lg" to={`/${lang}/docs`}>
              {t("landing.heroCtaDocs")}
            </Link>
          </div>

          <figure className="hero-terminal">
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
              </p>
              <p className="term-line term-ok">
                <span aria-hidden="true">✓</span> {t("landing.heroTerminalListening")}
              </p>
              <p className="term-line term-ok">
                <span aria-hidden="true">✓</span> {t("landing.heroTerminalOpened")}
              </p>
            </div>
          </figure>

          <ul className="hero-pills" aria-label="platforms">
            {PLATFORM_KEYS.map((os) => (
              <li className="pill" key={os}>
                {os === "macos" ? platforms.macos : os === "windows" ? platforms.windows : platforms.linux}
              </li>
            ))}
          </ul>
        </div>
      </section>

      <section className="stats" aria-label="facts">
        <div className="stats-inner">
          {Object.entries(
            t("landing.stats", { returnObjects: true }) as Record<string, { value: string; label: string }>,
          ).map(([id, s]) => (
            <div className="stat" key={s.label}>
              <span className="stat-value">{id === "s3" ? String(marketTotal) : s.value}</span>
              <span className="stat-label">{id === "s3" ? t("landing.marketStat") : s.label}</span>
            </div>
          ))}
        </div>
      </section>

      <section className="features" id="features">
        <div className="section-inner">
          <p className="eyebrow">
            <span className="eyebrow-index">01</span>
            {t("landing.eyebrow")}
          </p>
          <h2>{t("landing.featureTitle")}</h2>
          <p className="subtle">{t("landing.featureSubtitle")}</p>
          <div className="feature-grid">
            {Object.entries(features).map(([key, f]) => {
              const Icon = FEATURE_ICONS[key as keyof typeof FEATURE_ICONS] ?? Box;
              return (
                <article className="feature-card" key={key}>
                  <span className="feature-icon" aria-hidden="true">
                    <Icon size={20} />
                  </span>
                  <h3>{f.title}</h3>
                  <p>{f.desc}</p>
                </article>
              );
            })}
          </div>
        </div>
      </section>

      <WorkflowSteps />
      <DownloadCTA variant="full" lang={lang} />
    </>
  );
}
