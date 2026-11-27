#!/usr/bin/env bun
/**
 * Run flowy18_wip fast static checks automatically on pre-push when any pushed
 * commit modifies files under flowy18_wip/ or .github/workflows/flowy18-ui.yml.
 */
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ZERO_OID_RE = /^0+$/;

export function parsePushLines(stdinText) {
  const updates = [];
  for (const line of stdinText.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const [localRef, localOid, remoteRef, remoteOid] = trimmed.split(/\s+/);
    if (!localOid || !remoteOid || ZERO_OID_RE.test(localOid)) continue;
    updates.push({ localRef, localOid, remoteRef, remoteOid });
  }
  return updates;
}

export function touchesFlowy18Paths(paths) {
  return paths.some(
    (p) => p.startsWith("flowy18_wip/") || p === ".github/workflows/flowy18-ui.yml",
  );
}

function runGit(cwd, args, { allowFailure = false } = {}) {
  const result = spawnSync("git", args, {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  if (result.error) throw result.error;
  if (result.status !== 0 && !allowFailure) {
    throw new Error(`git ${args.join(" ")} failed: ${result.stderr.trim() || result.stdout.trim()}`);
  }
  return { ok: result.status === 0, stdout: result.stdout };
}

function changedFilesForPushUpdate(repoRoot, remoteName, update) {
  if (!ZERO_OID_RE.test(update.remoteOid)) {
    const diff = runGit(
      repoRoot,
      ["diff", "--name-only", `${update.remoteOid}..${update.localOid}`],
      { allowFailure: true },
    );
    if (diff.ok) {
      return diff.stdout.split(/\r?\n/).filter(Boolean);
    }
  }
  const remoteSel = remoteName && !remoteName.includes("://")
    ? `--remotes=${remoteName}`
    : "--remotes";
  const revList = runGit(repoRoot, ["rev-list", update.localOid, "--not", remoteSel], {
    allowFailure: true,
  });
  if (!revList.ok) return [];
  const files = new Set();
  for (const sha of revList.stdout.split(/\r?\n/).filter(Boolean)) {
    const show = runGit(repoRoot, ["diff-tree", "--no-commit-id", "--name-only", "-r", sha], {
      allowFailure: true,
    });
    if (show.ok) {
      for (const f of show.stdout.split(/\r?\n/).filter(Boolean)) files.add(f);
    }
  }
  return [...files];
}

export function shouldTriggerFlowy18Check({ updates, listChangedFiles }) {
  for (const update of updates) {
    if (touchesFlowy18Paths(listChangedFiles(update))) return true;
  }
  return false;
}

export function runPrePushCheck({ repoRoot, remoteName, stdinText, env = process.env, runner }) {
  const updates = parsePushLines(stdinText);
  if (!updates.length) return 0;

  const fastScript = join(repoRoot, "flowy18_wip", "scripts", "check-fast.mjs");
  if (!existsSync(fastScript)) return 0;

  const needsCheck = shouldTriggerFlowy18Check({
    updates,
    listChangedFiles: (u) => changedFilesForPushUpdate(repoRoot, remoteName, u),
  });
  if (!needsCheck) return 0;

  if (env.FLOWY_SKIP_PREPUSH_CHECK === "1") {
    console.warn("[pre-push] WARN: skipped flowy18 fast check because FLOWY_SKIP_PREPUSH_CHECK=1");
    return 0;
  }

  if (runner) return runner(fastScript);
  const res = spawnSync(process.execPath, [fastScript], {
    cwd: repoRoot,
    stdio: "inherit",
    windowsHide: true,
  });
  return res.status ?? 1;
}

const invokedPath = process.argv[1] ? resolve(process.argv[1]) : "";
if (import.meta.main || invokedPath === fileURLToPath(import.meta.url)) {
  const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
  const remoteName = process.argv[2] ?? "origin";
  const stdinText = readFileSync(0, "utf8");
  process.exitCode = runPrePushCheck({ repoRoot, remoteName, stdinText });
}
