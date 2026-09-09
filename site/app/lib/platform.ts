import type { Language } from "../i18n";

export type TargetOS = "macos" | "windows" | "linux";
export type TargetArch = "aarch64" | "x86_64";

export interface DetectedPlatform {
  os: TargetOS;
  arch: TargetArch;
}

/** Download host: the VPS serves prebuilt binaries over HTTP
 * (Caddy :8306 → http://111.170.173.22:10014/downloads/). Replace
 * `DOWNLOAD_HOST` when the release server moves.
 */
const DOWNLOAD_HOST = "http://111.170.173.22:10014";

/** GitHub repository (source only — binaries ship from the VPS). */
const GITHUB_REPO = "Michael-Lfx/allo";

/** Asset file name convention produced by the release build. */
function assetName(version: string, p: DetectedPlatform): string {
  const target = `${p.os}-${p.arch}`;
  const tag = version === "latest" ? "latest" : `v${version}`;
  return `flowy-agent-store-${tag}-${target}.zip`;
}

export function detectPlatform(): DetectedPlatform {
  if (typeof window === "undefined") return { os: "macos", arch: "aarch64" };

  const nav = window.navigator;
  const ua = (nav.userAgent || "").toLowerCase();
  const uaData = (nav as unknown as {
    userAgentData?: { platform?: string; architecture?: string };
  }).userAgentData;
  // `navigator.platform` is the most reliable signal inside embedded webviews
  // (IDE previews, Tauri, etc.) where the User-Agent string is often stripped
  // of OS tokens. Fall back to Client Hints, then the raw UA.
  const platformHint = (
    (nav as unknown as { platform?: string }).platform ||
    uaData?.platform ||
    ""
  ).toLowerCase();

  let os: TargetOS = "linux";
  if (/mac|iphone|ipad|ipod/.test(platformHint) || /mac os x|macintosh/.test(ua))
    os = "macos";
  else if (/win/.test(platformHint) || /windows nt/.test(ua)) os = "windows";

  let arch: TargetArch = "x86_64";
  if (uaData?.architecture) {
    // Client Hints expose the real CPU arch (e.g. "arm" on Apple Silicon).
    arch = /arm/.test(uaData.architecture) ? "aarch64" : "x86_64";
  } else if (/aarch64|arm64|\(arm;|arm;|\(arm\)/.test(ua) || /arm/.test(platformHint)) {
    arch = "aarch64";
  }
  return { os, arch };
}

/** Direct download URL for a platform's asset on the VPS download host. */
export function releaseAssetUrl(version: string, p: DetectedPlatform): string {
  const file = assetName(version, p);
  return `${DOWNLOAD_HOST}/downloads/${file}`;
}

/** VPS download directory (file listing / all published assets). */
export function releasesPageUrl(): string {
  return `${DOWNLOAD_HOST}/downloads/`;
}

export function githubUrl(): string {
  return `https://github.com/${GITHUB_REPO}`;
}

export const PLATFORM_LABELS: Record<TargetOS, Record<Language, string>> = {
  macos: { "zh-CN": "macOS", "en-US": "macOS" },
  windows: { "zh-CN": "Windows", "en-US": "Windows" },
  linux: { "zh-CN": "Linux", "en-US": "Linux" },
};

export const ARCH_LABELS: Record<TargetArch, Record<Language, string>> = {
  aarch64: { "zh-CN": "Apple 芯片", "en-US": "Apple silicon" },
  x86_64: { "zh-CN": "Intel / x64", "en-US": "Intel / x64" },
};
