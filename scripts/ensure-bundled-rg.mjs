#!/usr/bin/env bun
/**
 * Download a target-matched ripgrep binary into apps/desktop/resources/bin/
 * so Tauri packages it as `<resource_dir>/bin/rg[.exe]`.
 *
 * Prefer Tauri hook env (TAURI_ENV_TARGET_TRIPLE / ARCH / PLATFORM) so
 * cross-compiles (e.g. x86_64-apple-darwin on an arm64 runner) get the
 * correct slice. Falls back to process.platform / process.arch.
 *
 * Idempotent unless ENSURE_BUNDLED_RG_FORCE=1 or the on-disk binary's
 * recorded target does not match the requested target.
 * Uses the host `tar` (available on modern Windows / macOS / Linux).
 */
import { access, chmod, copyFile, mkdir, readdir, rm, stat, writeFile, readFile } from "node:fs/promises";
import { createWriteStream } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { pipeline } from "node:stream/promises";
import { spawnSync } from "node:child_process";

const RG_VERSION = "15.2.0";
const __dirname = dirname(fileURLToPath(import.meta.url));
const root = join(__dirname, "..");
const outDir = join(root, "apps", "desktop", "resources", "bin");
const TARGET_MARKER = ".rg-target";

/** Map Tauri/Cargo arch names onto Node's process.arch vocabulary. */
export function normalizeArch(arch) {
  if (!arch) return null;
  const a = arch.toLowerCase();
  if (a === "x86_64" || a === "amd64" || a === "x64") return "x64";
  if (a === "aarch64" || a === "arm64") return "arm64";
  return a;
}

/** Map Tauri platform names onto Node's process.platform vocabulary. */
export function normalizePlatform(platform) {
  if (!platform) return null;
  const p = platform.toLowerCase();
  if (p === "windows" || p === "win32") return "win32";
  if (p === "darwin" || p === "macos" || p === "mac") return "darwin";
  if (p === "linux") return "linux";
  return p;
}

/**
 * Resolve the ripgrep download target for this build.
 * @param {NodeJS.ProcessEnv} [env]
 * @param {{ platform?: string, arch?: string }} [host]
 */
export function resolveRgTarget(env = process.env, host = process) {
  const triple = (env.TAURI_ENV_TARGET_TRIPLE || "").trim();
  if (triple === "universal-apple-darwin") {
    return { platform: "darwin", arch: "universal", triple, binary: "rg" };
  }

  let platform = normalizePlatform(env.TAURI_ENV_PLATFORM);
  let arch = normalizeArch(env.TAURI_ENV_ARCH);

  if (triple) {
    const parts = triple.split("-");
    if (!arch && parts[0]) arch = normalizeArch(parts[0]);
    if (!platform) {
      if (triple.includes("apple-darwin") || triple.includes("macos")) platform = "darwin";
      else if (triple.includes("windows")) platform = "win32";
      else if (triple.includes("linux")) platform = "linux";
    }
  }

  platform = platform || normalizePlatform(host.platform) || host.platform;
  arch = arch || normalizeArch(host.arch) || host.arch;

  const binary = platform === "win32" ? "rg.exe" : "rg";
  const resolvedTriple =
    triple ||
    (platform === "darwin"
      ? `${arch === "arm64" ? "aarch64" : "x86_64"}-apple-darwin`
      : platform === "win32"
        ? `${arch === "arm64" ? "aarch64" : "x86_64"}-pc-windows-msvc`
        : `${arch === "arm64" ? "aarch64" : "x86_64"}-unknown-linux-gnu`);

  return { platform, arch, triple: resolvedTriple, binary };
}

export function platformAsset(platform, arch) {
  const base = `https://github.com/BurntSushi/ripgrep/releases/download/${RG_VERSION}`;

  if (platform === "win32" && arch === "x64") {
    return {
      url: `${base}/ripgrep-${RG_VERSION}-x86_64-pc-windows-msvc.zip`,
      binary: "rg.exe",
      archiveName: `ripgrep-${RG_VERSION}.zip`,
    };
  }
  if (platform === "win32" && arch === "arm64") {
    return {
      url: `${base}/ripgrep-${RG_VERSION}-aarch64-pc-windows-msvc.zip`,
      binary: "rg.exe",
      archiveName: `ripgrep-${RG_VERSION}.zip`,
    };
  }
  if (platform === "linux" && arch === "x64") {
    return {
      url: `${base}/ripgrep-${RG_VERSION}-x86_64-unknown-linux-musl.tar.gz`,
      binary: "rg",
      archiveName: `ripgrep-${RG_VERSION}.tar.gz`,
    };
  }
  if (platform === "linux" && arch === "arm64") {
    return {
      url: `${base}/ripgrep-${RG_VERSION}-aarch64-unknown-linux-gnu.tar.gz`,
      binary: "rg",
      archiveName: `ripgrep-${RG_VERSION}.tar.gz`,
    };
  }
  if (platform === "darwin" && arch === "x64") {
    return {
      url: `${base}/ripgrep-${RG_VERSION}-x86_64-apple-darwin.tar.gz`,
      binary: "rg",
      archiveName: `ripgrep-${RG_VERSION}.tar.gz`,
    };
  }
  if (platform === "darwin" && arch === "arm64") {
    return {
      url: `${base}/ripgrep-${RG_VERSION}-aarch64-apple-darwin.tar.gz`,
      binary: "rg",
      archiveName: `ripgrep-${RG_VERSION}.tar.gz`,
    };
  }
  throw new Error(`unsupported platform for bundled ripgrep: ${platform}/${arch}`);
}

async function pathExists(path) {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

async function download(url, dest) {
  const headers = { "User-Agent": "allo/ensure-bundled-rg" };
  if (process.env.GITHUB_TOKEN) {
    headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
    headers.Accept = "application/octet-stream";
  }
  const res = await fetch(url, { headers });
  if (!res.ok || !res.body) {
    throw new Error(`download failed HTTP ${res.status} for ${url}`);
  }
  await pipeline(res.body, createWriteStream(dest));
}

async function findFile(dir, targetName) {
  for (const name of await readdir(dir)) {
    const p = join(dir, name);
    const s = await stat(p);
    if (s.isDirectory()) {
      const hit = await findFile(p, targetName);
      if (hit) return hit;
    } else if (name === targetName) {
      return p;
    }
  }
  return null;
}

async function extractBinary(asset, extractDir) {
  const archive = join(outDir, asset.archiveName);
  await rm(extractDir, { recursive: true, force: true });
  await mkdir(extractDir, { recursive: true });

  console.log(`[ensure-bundled-rg] downloading ${asset.url}`);
  await download(asset.url, archive);

  const tar = spawnSync("tar", ["-xf", archive, "-C", extractDir], {
    encoding: "utf8",
  });
  if (tar.status !== 0) {
    throw new Error(`tar extract failed: ${tar.stderr || tar.stdout || tar.status}`);
  }

  const found = await findFile(extractDir, asset.binary);
  if (!found) {
    throw new Error(`${asset.binary} not found after extracting ${archive}`);
  }
  await rm(archive, { force: true });
  return found;
}

async function installSingleArch(platform, arch, dest) {
  const asset = platformAsset(platform, arch);
  const extractDir = join(outDir, ".extract-tmp");
  const found = await extractBinary(asset, extractDir);
  await copyFile(found, dest);
  if (platform !== "win32") {
    await chmod(dest, 0o755);
  }
  await rm(extractDir, { recursive: true, force: true });
}

async function installUniversalDarwin(dest) {
  const extractDir = join(outDir, ".extract-tmp");
  const armAsset = platformAsset("darwin", "arm64");
  const intelAsset = platformAsset("darwin", "x64");
  const armBin = await extractBinary(armAsset, join(extractDir, "arm"));
  const intelBin = await extractBinary(intelAsset, join(extractDir, "intel"));
  const lipo = spawnSync("lipo", ["-create", armBin, intelBin, "-output", dest], {
    encoding: "utf8",
  });
  if (lipo.status !== 0) {
    throw new Error(`lipo failed: ${lipo.stderr || lipo.stdout || lipo.status}`);
  }
  await chmod(dest, 0o755);
  await rm(extractDir, { recursive: true, force: true });
}

async function recordedTargetMatches(markerPath, triple) {
  if (!(await pathExists(markerPath))) return false;
  const recorded = (await readFile(markerPath, "utf8")).trim();
  return recorded === triple;
}

async function main() {
  const target = resolveRgTarget();
  const dest = join(outDir, target.binary);
  const markerPath = join(outDir, TARGET_MARKER);
  const force = process.env.ENSURE_BUNDLED_RG_FORCE === "1";

  await mkdir(outDir, { recursive: true });
  await Bun.write(join(outDir, ".gitkeep"), "");

  if (!force && (await pathExists(dest)) && (await recordedTargetMatches(markerPath, target.triple))) {
    console.log(`[ensure-bundled-rg] already present for ${target.triple}: ${dest}`);
    return;
  }

  if (target.arch === "universal") {
    await installUniversalDarwin(dest);
  } else {
    await installSingleArch(target.platform, target.arch, dest);
  }

  await writeFile(markerPath, `${target.triple}\n`, "utf8");
  console.log(`[ensure-bundled-rg] installed ${target.triple}: ${dest}`);
}

if (import.meta.main) {
  main().catch((err) => {
    console.error(`[ensure-bundled-rg] ${err?.stack || err}`);
    process.exit(1);
  });
}
