#!/usr/bin/env bun
/**
 * Download a platform-matched ripgrep binary into apps/desktop/resources/bin/
 * so Tauri packages it as `<resource_dir>/bin/rg[.exe]`.
 *
 * Idempotent unless ENSURE_BUNDLED_RG_FORCE=1.
 * Uses the host `tar` (available on modern Windows / macOS / Linux).
 */
import { access, chmod, copyFile, mkdir, readdir, rm, stat } from "node:fs/promises";
import { createWriteStream } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { pipeline } from "node:stream/promises";
import { spawnSync } from "node:child_process";

const RG_VERSION = "15.2.0";
const __dirname = dirname(fileURLToPath(import.meta.url));
const root = join(__dirname, "..");
const outDir = join(root, "apps", "desktop", "resources", "bin");

function platformAsset() {
  const platform = process.platform;
  const arch = process.arch;
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

async function main() {
  const asset = platformAsset();
  const dest = join(outDir, asset.binary);
  const force = process.env.ENSURE_BUNDLED_RG_FORCE === "1";

  await mkdir(outDir, { recursive: true });
  // Keep an empty marker so Tauri resource globs don't fail if download is skipped in CI.
  await Bun.write(join(outDir, ".gitkeep"), "");

  if (!force && (await pathExists(dest))) {
    console.log(`[ensure-bundled-rg] already present: ${dest}`);
    return;
  }

  const archive = join(outDir, asset.archiveName);
  const extractDir = join(outDir, ".extract-tmp");
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
  await copyFile(found, dest);
  if (process.platform !== "win32") {
    await chmod(dest, 0o755);
  }

  await rm(archive, { force: true });
  await rm(extractDir, { recursive: true, force: true });
  console.log(`[ensure-bundled-rg] installed: ${dest}`);
}

main().catch((err) => {
  console.error(`[ensure-bundled-rg] ${err?.stack || err}`);
  process.exit(1);
});
