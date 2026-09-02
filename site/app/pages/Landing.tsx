import { Link, useParams } from "react-router";
import { useTranslation } from "react-i18next";
import { Box, HardDrive, Layers, Terminal } from "lucide-react";

import type { Language } from "../i18n";
import WorkflowSteps from "../components/WorkflowSteps";
import DownloadCTA from "../components/DownloadCTA";

const FEATURE_ICONS = { singleBinary: Box, localFirst: HardDrive, catalog: Layers, cliUi: Terminal };

export default function Landing() {
  const { lang: raw } = useParams();
  const lang: Language = raw === "en-US" ? "en-US" : "zh-CN";
  const { t } = useTranslation();
  const features = t("landing.features", { returnObjects: true }) as Record<
    string,
    { title: string; desc: string }
  >;

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
        </div>
      </section>

      <section className="features" id="features">
        <div className="section-inner">
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
