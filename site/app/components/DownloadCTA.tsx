import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Download } from "lucide-react";

import type { Language } from "../i18n";
import { revealDelay } from "../lib/effects";
import {
  ARCH_LABELS,
  detectPlatform,
  type DetectedPlatform,
  PLATFORM_LABELS,
  type TargetArch,
  type TargetOS,
  installScriptUrl,
  releaseAssetUrl,
  releasesPageUrl,
} from "../lib/platform";
import CopyButton from "./CopyButton";

// Only the currently shipped target is downloadable; others return once
// their builds are published (see the assets dir on the download host).
const PLATFORMS: { os: TargetOS; arch: TargetArch }[] = [{ os: "windows", arch: "x86_64" }];

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

  const detectedUrl = detected ? releaseAssetUrl("latest", detected) : releasesPageUrl();
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
            {t("landing.download.primaryCta", { os: detectedLabel })}
          </a>
          {detected && <span className="detect-note">{t("landing.download.detectNote")}</span>}
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
            {PLATFORMS.map((p) => (
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
