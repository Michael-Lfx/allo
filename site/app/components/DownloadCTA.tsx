import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Download } from "lucide-react";

import type { Language } from "../i18n";
import {
  ARCH_LABELS,
  detectPlatform,
  type DetectedPlatform,
  PLATFORM_LABELS,
  type TargetArch,
  type TargetOS,
  releaseAssetUrl,
  releasesPageUrl,
} from "../lib/platform";

const PLATFORMS: { os: TargetOS; arch: TargetArch }[] = [
  { os: "macos", arch: "aarch64" },
  { os: "macos", arch: "x86_64" },
  { os: "windows", arch: "x86_64" },
  { os: "linux", arch: "x86_64" },
  { os: "linux", arch: "aarch64" },
];

export default function DownloadCTA({
  variant = "full",
  lang,
}: {
  variant?: "full" | "compact";
  lang: Language;
}) {
  const { t } = useTranslation();
  const [detected, setDetected] = useState<DetectedPlatform | null>(null);

  useEffect(() => {
    setDetected(detectPlatform());
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
        <p className="eyebrow eyebrow-center">
          <span className="eyebrow-index">03</span>
          {t("landing.eyebrow")}
        </p>
        <h2>{t("landing.downloadTitle")}</h2>
        <p className="subtle">{t("landing.downloadSubtitle")}</p>

        <div className="download-actions">
          <a className="btn btn-primary btn-lg" href={detectedUrl}>
            <Download size={18} />
            {t("landing.download.primaryCta", { os: detectedLabel })}
          </a>
          {detected && <span className="detect-note">{t("landing.download.detectNote")}</span>}
        </div>

        <details className="platforms">
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

        <p className="release-note">
          <a href={releasesPageUrl()}>{t("landing.download.releaseNote")}</a>
        </p>
      </div>
    </section>
  );
}
