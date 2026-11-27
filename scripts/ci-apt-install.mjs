#!/usr/bin/env bun
/**
 * Install required Ubuntu packages in Linux CI and release jobs with:
 *
 *   1. Preinstalled fast path — queries dpkg locally first and skips all APT
 *      network traffic when every requested package is already installed.
 *   2. Ubuntu-only APT source whitelist — builds a temporary, non-destructive
 *      Dir::Etc::sourcelist / Dir::Etc::sourceparts tree containing only
 *      official Ubuntu repositories (ubuntu.com hosts and validated local
 *      mirrorlists such as /etc/apt/apt-mirrors.txt), isolating CI from
 *      unrelated third-party runner repos (e.g. packages.microsoft.com 403s).
 *   3. Fail-closed retries — retries apt-get update + install against the
 *      isolated Ubuntu sources (never --fix-missing), fails if zero Ubuntu
 *      sources survive filtering, and verifies any --verify-bin commands exist
 *      on PATH before exiting 0.
 *
 * Usage:
 *   bun scripts/ci-apt-install.mjs [--verify-bin <cmd> ...] [--attempts <n>] [--retry-delay-ms <ms>] [--] <pkg> ...
 */

import { spawnSync } from "node:child_process";
import {
  accessSync,
  chmodSync,
  constants as fsConstants,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join } from "node:path";

const DEFAULT_ATTEMPTS = 3;
const DEFAULT_RETRY_DELAY_MS = 5000;
const DEFAULT_APT_DIR = "/etc/apt";

export function parseCliArgs(argv) {
  const packages = [];
  const verifyBins = [];
  let attempts = DEFAULT_ATTEMPTS;
  let retryDelayMs = DEFAULT_RETRY_DELAY_MS;
  let afterDoubleDash = false;

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (!afterDoubleDash && arg === "--") {
      afterDoubleDash = true;
      continue;
    }
    if (!afterDoubleDash && arg === "--verify-bin") {
      const value = argv[++i];
      if (!value || value.startsWith("-")) {
        throw new Error("--verify-bin requires a binary name");
      }
      verifyBins.push(value);
      continue;
    }
    if (!afterDoubleDash && arg === "--attempts") {
      const raw = argv[++i];
      const parsed = Number.parseInt(raw ?? "", 10);
      if (!Number.isInteger(parsed) || parsed < 1) {
        throw new Error(`--attempts must be a positive integer, got: ${raw ?? "(missing)"}`);
      }
      attempts = parsed;
      continue;
    }
    if (!afterDoubleDash && arg === "--retry-delay-ms") {
      const raw = argv[++i];
      const parsed = Number.parseInt(raw ?? "", 10);
      if (!Number.isInteger(parsed) || parsed < 0) {
        throw new Error(`--retry-delay-ms must be a non-negative integer, got: ${raw ?? "(missing)"}`);
      }
      retryDelayMs = parsed;
      continue;
    }
    if (!afterDoubleDash && arg.startsWith("-")) {
      throw new Error(`unknown option: ${arg}`);
    }
    packages.push(arg);
  }

  if (packages.length === 0) {
    throw new Error("at least one APT package name is required");
  }

  return { packages, verifyBins, attempts, retryDelayMs };
}

export function isUbuntuHttpUri(rawUri) {
  let url;
  try {
    url = new URL(rawUri);
  } catch {
    return false;
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    return false;
  }
  const host = url.hostname.toLowerCase();
  return host === "ubuntu.com" || host.endsWith(".ubuntu.com");
}

function defaultReadMirrorFile(filePath) {
  return readFileSync(filePath, "utf8");
}

/**
 * Accept only official Ubuntu HTTP(S) repositories or local APT mirrorlists
 * (`mirror+file:...`) whose every declared mirror entry resolves to Ubuntu.
 */
export function isAllowedUbuntuUri(rawUri, { readMirrorFile = defaultReadMirrorFile } = {}) {
  if (isUbuntuHttpUri(rawUri)) {
    return true;
  }
  if (!rawUri.startsWith("mirror+file:")) {
    return false;
  }

  const rawPath = rawUri.slice("mirror+file:".length);
  const mirrorPath = rawPath.startsWith("///") ? rawPath.slice(2) : rawPath;
  if (!mirrorPath.startsWith("/")) {
    return false;
  }

  let content;
  try {
    content = readMirrorFile(mirrorPath);
  } catch {
    return false;
  }

  const mirrorUris = content
    .split(/\r?\n/)
    .map((line) => line.replace(/#.*$/, "").trim())
    .filter((line) => line.length > 0)
    .map((line) => line.split(/\s+/)[0]);

  return mirrorUris.length > 0 && mirrorUris.every((uri) => isUbuntuHttpUri(uri));
}

export function parseOneLineSource(rawLine) {
  const withoutComment = rawLine.replace(/#.*$/, "").trim();
  if (!withoutComment) {
    return null;
  }
  const match = withoutComment.match(/^(deb(?:-src)?)\s+(.*)$/);
  if (!match) {
    return null;
  }
  const type = match[1];
  let rest = match[2].trim();
  if (rest.startsWith("[")) {
    const closeIdx = rest.indexOf("]");
    if (closeIdx === -1) {
      return null;
    }
    rest = rest.slice(closeIdx + 1).trim();
  }
  const uri = rest.split(/\s+/)[0];
  if (!uri) {
    return null;
  }
  return { type, uri, line: withoutComment };
}

export function filterOneLineSources(content, isAllowed = isAllowedUbuntuUri) {
  const keptLines = [];
  const droppedUris = [];

  for (const rawLine of content.split(/\r?\n/)) {
    const parsed = parseOneLineSource(rawLine);
    if (!parsed) {
      continue;
    }
    if (parsed.type === "deb" && isAllowed(parsed.uri)) {
      keptLines.push(parsed.line);
    } else {
      droppedUris.push(parsed.uri);
    }
  }

  return {
    content: keptLines.length > 0 ? `${keptLines.join("\n")}\n` : "",
    keptCount: keptLines.length,
    droppedUris,
  };
}

function parseDeb822Fields(stanza) {
  const fields = new Map();
  let currentKey = null;

  for (const rawLine of stanza.split("\n")) {
    if (/^\s*#/.test(rawLine) || rawLine.trim().length === 0) {
      continue;
    }
    if (/^[ \t]/.test(rawLine)) {
      if (currentKey !== null) {
        fields.set(currentKey, `${fields.get(currentKey)}\n${rawLine.trim()}`);
      }
      continue;
    }
    const match = rawLine.match(/^([A-Za-z0-9-]+)\s*:\s*(.*)$/);
    if (match) {
      currentKey = match[1].toLowerCase();
      fields.set(currentKey, match[2].trim());
    }
  }

  return fields;
}

export function filterDeb822Sources(content, isAllowed = isAllowedUbuntuUri) {
  const normalized = content.replace(/\r\n/g, "\n");
  const stanzas = normalized.split(/\n[ \t]*\n+/);
  const keptStanzas = [];
  const droppedUris = [];

  for (const rawStanza of stanzas) {
    const fields = parseDeb822Fields(rawStanza);
    if (fields.size === 0) {
      continue;
    }
    const enabled = (fields.get("enabled") ?? "yes").trim().toLowerCase();
    if (enabled === "no") {
      continue;
    }
    const types = (fields.get("types") ?? "")
      .trim()
      .split(/\s+/)
      .filter(Boolean);
    const uris = (fields.get("uris") ?? "")
      .trim()
      .split(/\s+/)
      .filter(Boolean);

    if (types.includes("deb") && uris.length > 0 && uris.every((uri) => isAllowed(uri))) {
      keptStanzas.push(rawStanza.trim());
    } else if (uris.length > 0) {
      droppedUris.push(...uris);
    }
  }

  return {
    content: keptStanzas.length > 0 ? `${keptStanzas.join("\n\n")}\n` : "",
    keptCount: keptStanzas.length,
    droppedUris,
  };
}

/**
 * Create a temporary APT config containing only official Ubuntu sources.
 * Leaves the host's `/etc/apt` completely untouched.
 */
export function prepareUbuntuOnlyAptConfig({
  aptDir = DEFAULT_APT_DIR,
  tmpParent = tmpdir(),
  readMirrorFile = defaultReadMirrorFile,
} = {}) {
  const isAllowed = (uri) => isAllowedUbuntuUri(uri, { readMirrorFile });
  const tmpDir = mkdtempSync(join(tmpParent, "ci-apt-ubuntu-"));
  const sourceListPath = join(tmpDir, "sources.list");
  const sourcePartsDir = join(tmpDir, "sources.list.d");

  try {
    mkdirSync(sourcePartsDir, { recursive: true });
    // Ensure _apt (unprivileged acquire user) can traverse the temporary config dir.
    try {
      chmodSync(tmpDir, 0o755);
      chmodSync(sourcePartsDir, 0o755);
    } catch {
      // Non-POSIX filesystems (e.g. Windows in unit tests) may ignore chmod.
    }

    let keptCount = 0;
    const keptFiles = [];
    const dropped = [];

    const hostSourcesList = join(aptDir, "sources.list");
    let filteredMainList = "";
    if (existsSync(hostSourcesList) && statSync(hostSourcesList).isFile()) {
      const result = filterOneLineSources(readFileSync(hostSourcesList, "utf8"), isAllowed);
      filteredMainList = result.content;
      keptCount += result.keptCount;
      if (result.keptCount > 0) {
        keptFiles.push("sources.list");
      }
      for (const uri of result.droppedUris) {
        dropped.push({ file: "sources.list", uri });
      }
    }
    writeFileSync(sourceListPath, filteredMainList, { mode: 0o644 });

    const hostPartsDir = join(aptDir, "sources.list.d");
    if (existsSync(hostPartsDir) && statSync(hostPartsDir).isDirectory()) {
      const entries = readdirSync(hostPartsDir).sort();
      for (const name of entries) {
        if (name.startsWith(".")) {
          continue;
        }
        const fullPath = join(hostPartsDir, name);
        let st;
        try {
          st = statSync(fullPath);
        } catch {
          continue;
        }
        if (!st.isFile()) {
          continue;
        }

        let result = null;
        if (name.endsWith(".list")) {
          result = filterOneLineSources(readFileSync(fullPath, "utf8"), isAllowed);
        } else if (name.endsWith(".sources")) {
          result = filterDeb822Sources(readFileSync(fullPath, "utf8"), isAllowed);
        } else {
          continue;
        }

        for (const uri of result.droppedUris) {
          dropped.push({ file: `sources.list.d/${name}`, uri });
        }
        if (result.keptCount > 0) {
          writeFileSync(join(sourcePartsDir, name), result.content, { mode: 0o644 });
          keptCount += result.keptCount;
          keptFiles.push(`sources.list.d/${name}`);
        }
      }
    }

    if (keptCount === 0) {
      throw new Error(
        `no official Ubuntu APT sources found under ${aptDir}; refusing to run apt-get with an empty source list`,
      );
    }

    return {
      tmpDir,
      sourceListPath,
      sourcePartsDir,
      keptCount,
      keptFiles,
      dropped,
      cleanup: () => rmSync(tmpDir, { recursive: true, force: true }),
    };
  } catch (err) {
    rmSync(tmpDir, { recursive: true, force: true });
    throw err;
  }
}

function defaultRunCommand(command, args, options = {}) {
  return spawnSync(command, args, {
    encoding: "utf8",
    stdio: options.stdio ?? "inherit",
  });
}

export function findMissingPackages(packages, runCommand = defaultRunCommand) {
  const missing = [];
  for (const pkg of packages) {
    const res = runCommand("dpkg-query", ["-W", "-f=${Status}\n", "--", pkg], {
      stdio: ["ignore", "pipe", "ignore"],
    });
    const installed =
      res.status === 0 &&
      String(res.stdout ?? "")
        .split(/\r?\n/)
        .some((line) => line.trim() === "install ok installed");
    if (!installed) {
      missing.push(pkg);
    }
  }
  return missing;
}

export function findExecutableInPath(binName, envPath = process.env.PATH ?? "") {
  const pathDirs = envPath.split(delimiter).filter(Boolean);
  const candidates =
    process.platform === "win32"
      ? [binName, `${binName}.exe`, `${binName}.cmd`, `${binName}.bat`]
      : [binName];

  for (const dir of pathDirs) {
    for (const candidate of candidates) {
      const fullPath = join(dir, candidate);
      try {
        if (!statSync(fullPath).isFile()) {
          continue;
        }
        if (process.platform !== "win32") {
          accessSync(fullPath, fsConstants.X_OK);
        }
        return fullPath;
      } catch {
        // Keep scanning PATH entries.
      }
    }
  }
  return null;
}

export function verifyRequiredBins(verifyBins, findExecutable = findExecutableInPath) {
  const missingBins = verifyBins.filter((bin) => !findExecutable(bin));
  if (missingBins.length > 0) {
    throw new Error(`required binary not found on PATH: ${missingBins.join(", ")}`);
  }
}

function sleepSync(ms) {
  if (ms <= 0) {
    return;
  }
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

export function installAptPackages({
  packages,
  verifyBins = [],
  attempts = DEFAULT_ATTEMPTS,
  retryDelayMs = DEFAULT_RETRY_DELAY_MS,
  aptDir = DEFAULT_APT_DIR,
  tmpParent = tmpdir(),
  isRoot = typeof process.getuid === "function" && process.getuid() === 0,
  runCommand = defaultRunCommand,
  findExecutable = findExecutableInPath,
  readMirrorFile = defaultReadMirrorFile,
  sleep = sleepSync,
  log = console.log,
}) {
  const missing = findMissingPackages(packages, runCommand);
  if (missing.length === 0) {
    verifyRequiredBins(verifyBins, findExecutable);
    log(
      `[ci-apt-install] all ${packages.length} package(s) already installed (${packages.join(", ")}); skipping apt-get update/install`,
    );
    return { skipped: true, installed: [] };
  }

  log(
    `[ci-apt-install] missing ${missing.length}/${packages.length} package(s): ${missing.join(", ")}`,
  );

  const aptConfig = prepareUbuntuOnlyAptConfig({ aptDir, tmpParent, readMirrorFile });
  try {
    log(
      `[ci-apt-install] isolated Ubuntu-only APT sources (${aptConfig.keptCount} entry/entries from ${aptConfig.keptFiles.join(", ")}; skipped ${aptConfig.dropped.length} non-Ubuntu entry/entries)`,
    );
    for (const item of aptConfig.dropped) {
      log(`[ci-apt-install]   ignored third-party/non-deb source: ${item.file} -> ${item.uri}`);
    }

    const aptSourceArgs = [
      "-o",
      `Dir::Etc::sourcelist=${aptConfig.sourceListPath}`,
      "-o",
      `Dir::Etc::sourceparts=${aptConfig.sourcePartsDir}`,
      "-o",
      "APT::Get::List-Cleanup=0",
    ];

    const execApt = (prefixEnv, aptArgs) => {
      if (isRoot) {
        const command = prefixEnv.length > 0 ? "env" : "apt-get";
        const args = prefixEnv.length > 0 ? [...prefixEnv, "apt-get", ...aptArgs] : aptArgs;
        return runCommand(command, args, { stdio: "inherit" });
      }
      const args = prefixEnv.length > 0 ? ["env", ...prefixEnv, "apt-get", ...aptArgs] : ["apt-get", ...aptArgs];
      return runCommand("sudo", args, { stdio: "inherit" });
    };

    let installedOk = false;
    for (let attempt = 1; attempt <= attempts; attempt += 1) {
      const updateRes = execApt([], [...aptSourceArgs, "update"]);
      if (updateRes.status === 0) {
        const installRes = execApt(
          ["DEBIAN_FRONTEND=noninteractive"],
          [...aptSourceArgs, "install", "-y", "-o", "Acquire::Retries=3", "--", ...missing],
        );
        if (installRes.status === 0) {
          installedOk = true;
          break;
        }
      }
      if (attempt === attempts) {
        break;
      }
      log(
        `[ci-apt-install] apt-get failed (attempt ${attempt}/${attempts}); refreshing Ubuntu mirror metadata and retrying in ${retryDelayMs}ms...`,
      );
      sleep(retryDelayMs);
    }

    if (!installedOk) {
      throw new Error(
        `failed to install Ubuntu packages after ${attempts} attempt(s): ${missing.join(", ")}`,
      );
    }
  } finally {
    aptConfig.cleanup();
  }

  verifyRequiredBins(verifyBins, findExecutable);
  return { skipped: false, installed: missing };
}

if (import.meta.main) {
  try {
    const parsed = parseCliArgs(process.argv.slice(2));
    installAptPackages(parsed);
  } catch (err) {
    console.error(`[ci-apt-install] ERROR: ${err instanceof Error ? err.message : String(err)}`);
    process.exit(1);
  }
}
