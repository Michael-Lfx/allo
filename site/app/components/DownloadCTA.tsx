import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Download } from "lucide-react";

import type { Language } from "../i18n";
import { revealDelay } from "../lib/effects";
import {
  ARCH_LABELS,
  detectPlatform,
  type DetectedPlatform,
  isReleasedPlatform,
  PLATFORM_LABELS,
  RELEASED_PLATFORMS,
  installScriptUrl,
  releaseAssetUrl,
  releasesPageUrl,
} from "../lib/platform";
import CopyButton from "./CopyButton";

export default function DownloadCTA({
  variant = "full",
  lang,
}: {
  variant?: "full" | "compact";
  lang: Language;
}) {
  const { t } = useTranslation();
  const [detected, setDetected] = useState<DetectedPlatform | null>(null);
  const [oneLiner, setOneLiner] = useState("");

  useEffect(() => {
    setDetected(detectPlatform());
    // Absolute URL so `irm | iex` works from any terminal cwd.
    setOneLiner(`irm ${window.location.origin}${installScriptUrl()} | iex`);
  }, []);

  // Only point at a direct asset when the visitor's platform is actually
  // published; everyone else goes to the downloads page (a guessed asset URL
  // would 404 on macOS / Linux / ARM).
  const detectedReleased = detected !== null && isReleasedPlatform(detected);
  const detectedUrl = detected && detectedReleased ? releaseAssetUrl("latest", detected) : releasesPageUrl();
  const detectedLabel = detected
    ? `${PLATFORM_LABELS[detected.os][lang]} · ${ARCH_LABELS[detected.arch][lang]}`
    : "";

  if (variant === "compact") {
    return (
      <a className="btn btn-primary" href={detectedUrl}>
        <Download size={16} />
        {t("nav.download")}
      </a>
    );
  }

  return (
    <section className="download" id="download">
      <div className="download-inner">
        <p className="eyebrow eyebrow-center" data-reveal>
          <span className="eyebrow-index">03</span>
          {t("landing.eyebrow")}
        </p>
        <h2 data-reveal>{t("landing.downloadTitle")}</h2>
        <p className="subtle subtle-center" data-reveal>{t("landing.downloadSubtitle")}</p>

        <div className="download-actions" data-reveal style={revealDelay(120)}>
          <a className="btn btn-primary btn-lg btn-glow" href={detectedUrl}>
            <Download size={18} />
            {detectedReleased
              ? t("landing.download.primaryCta", { os: detectedLabel })
              : t("landing.download.fallbackCta")}
          </a>
          {detected && (
            <span className="detect-note">
              {detectedReleased
                ? t("landing.download.detectNote")
                : t("landing.download.unavailableNote")}
            </span>
          )}
        </div>

        <details className="platforms" data-reveal style={revealDelay(160)}>
          <summary>{t("landing.download.psTitle")}</summary>
          <div className="ps-block">
            <p className="ps-hint">{t("landing.download.psHint")}</p>
            <div className="ps-cmd">
              <code>{oneLiner || `irm ${installScriptUrl()} | iex`}</code>
              <CopyButton text={oneLiner || `irm ${installScriptUrl()} | iex`} label={t("landing.download.copy")} />
            </div>
            <p className="ps-note">
              <a href={installScriptUrl()} target="_blank" rel="noreferrer">
                {t("landing.download.psView")}
              </a>
            </p>
          </div>
        </details>

        <details className="platforms" data-reveal style={revealDelay(200)}>
          <summary>{t("landing.download.manual")}</summary>
          <div className="platform-grid">
            {RELEASED_PLATFORMS.map((p) => (
              <a key={`${p.os}-${p.arch}`} className="platform-card" href={releaseAssetUrl("latest", p)}>
                <span className="platform-os">{PLATFORM_LABELS[p.os][lang]}</span>
                <span className="platform-arch">{ARCH_LABELS[p.arch][lang]}</span>
              </a>
            ))}
          </div>
        </details>

        <p className="release-note" data-reveal style={revealDelay(260)}>
          <a href={releasesPageUrl()}>{t("landing.download.releaseNote")}</a>
        </p>
      </div>
    </section>
  );
}
