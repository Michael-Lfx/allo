/**
 * Windows preflight: warn when the checkout carries a Low integrity label.
 *
 * A checkout whose directory tree has a `Low` mandatory integrity label makes
 * every program built inside it run at Low integrity; Windows mandatory
 * integrity control then blocks write-up to `%TEMP%` and `%LOCALAPPDATA%`, so
 * the dev loop fails at link time (LNK1104), at backend bootstrap (a
 * PermissionDenied log-file panic) and at WebView2 profile creation
 * (0x8000FFFF / 0x800700AA).
 *
 * This preflight is deliberately warning-only: it never needs administrator
 * rights, never throws, never blocks the dev loop and never changes the exit
 * code. Background and the administrator fix live in
 * `docs/contributing/windows-low-integrity-dev-loop.md`.
 */
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join } from 'node:path';

const LOW_LABEL_MARKER = 'Low Mandatory Level';
const SKIP_ENV = 'NOMIFUN_SKIP_INTEGRITY_CHECK';
const DOCS_PATH = 'docs/contributing/windows-low-integrity-dev-loop.md';

/** True when `icacls` output reports a Low mandatory integrity label. */
export function outputCarriesLowLabel(output) {
  return typeof output === 'string' && output.includes(LOW_LABEL_MARKER);
}

/**
 * Probe the checkout root and the build directory for the Low label.
 *
 * Returns the offending paths (empty when clean). Non-Windows hosts, missing
 * paths and a failing `icacls` are all treated as "nothing to report".
 */
export function findLowIntegrityPaths({
  root,
  platform = process.platform,
  spawn = spawnSync,
  exists = existsSync,
} = {}) {
  if (platform !== 'win32') return [];
  const hits = [];
  for (const target of [root, join(root, 'build.noindex')]) {
    if (!exists(target)) continue;
    try {
      const result = spawn('icacls', [target], { encoding: 'utf8', windowsHide: true });
      const output = `${result?.stdout ?? ''}${result?.stderr ?? ''}`;
      if (outputCarriesLowLabel(output)) hits.push(target);
    } catch {
      // Warning-only preflight: a broken probe must never block the dev loop.
    }
  }
  return hits;
}

/**
 * Print one actionable warning when the Low label is present.
 *
 * Returns the offending paths so callers (and tests) can assert without
 * scraping the log text.
 */
export function warnOnLowIntegrityLabel({
  root,
  platform = process.platform,
  spawn = spawnSync,
  exists = existsSync,
  log = console.warn,
  env = process.env,
} = {}) {
  if (env?.[SKIP_ENV] === '1') return [];
  const hits = findLowIntegrityPaths({ root, platform, spawn, exists });
  if (hits.length === 0) return hits;

  log('[run-dev] WARNING: this checkout carries a Windows Low integrity label:');
  for (const hit of hits) log(`[run-dev]   ${hit}`);
  log(
    '[run-dev] Programs built here cannot write %TEMP%, the per-user data dir, or the WebView2 profile,',
  );
  log(
    '[run-dev] so the dev loop fails with LNK1104, a PermissionDenied log-file panic, or WebView2 0x8000FFFF.',
  );
  log('[run-dev] Fix in an administrator PowerShell:');
  log(`[run-dev]   icacls "${root}" /setintegritylevel Medium /T /C /Q`);
  log(`[run-dev] Details: ${DOCS_PATH}`);
  log(`[run-dev] Skip this check: ${SKIP_ENV}=1`);
  return hits;
}
