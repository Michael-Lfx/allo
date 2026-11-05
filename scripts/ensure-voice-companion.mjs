#!/usr/bin/env bun
/**
 * ensure-voice-companion — provision the local voice daemon for a source checkout.
 *
 * `bun run dev` builds only the Flowy binary: `voice-companion` is a
 * dev-dependency (lib only), so a fresh checkout has no daemon and the app
 * reports "voice service component missing". This preflight resolves the same
 * locations the backend resolver checks, rebuilds the daemon when it is missing
 * or older than the crate sources, and never blocks the dev loop on failure.
 *
 * Spec: docs/architecture/voice-agent/execution/dev-voice-companion-provisioning-fix-2026-11-04.zh.md
 */
import { spawnSync } from 'node:child_process';
import { existsSync, statSync } from 'node:fs';
import { dirname, join, posix, win32 } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const SKIP_ENV = 'NOMIFUN_SKIP_VOICE_COMPANION';
const SOURCE_ANCHORS = ['apps/voice-companion/src/main.rs', 'apps/voice-companion/Cargo.toml'];

/** The only accepted daemon names. */
export const COMPANION_NAMES = ['voice-companion', 'voice-companion.exe'];

const HOST_TRIPLES = {
  win32: ['x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc'],
  darwin: ['aarch64-apple-darwin', 'x86_64-apple-darwin'],
  linux: [
    'x86_64-unknown-linux-gnu',
    'aarch64-unknown-linux-gnu',
    'x86_64-unknown-linux-musl',
    'aarch64-unknown-linux-musl',
  ],
};

function joiner(platform) {
  return platform === 'win32' ? win32.join : posix.join;
}

function fileNames(platform) {
  return platform === 'win32' ? [COMPANION_NAMES[1]] : [COMPANION_NAMES[0]];
}

/** Directories of a source checkout that may contain a development build. */
export function candidateRoots({ root, platform = process.platform }) {
  const joinPath = joiner(platform);
  const roots = [];
  for (const base of ['target', 'build.noindex']) {
    for (const profile of ['debug', 'release']) roots.push(joinPath(root, base, profile));
  }
  for (const triple of HOST_TRIPLES[platform] ?? HOST_TRIPLES.linux) {
    for (const base of ['target', 'build.noindex']) {
      for (const profile of ['debug', 'release']) roots.push(joinPath(root, base, triple, profile));
    }
  }
  roots.push(joinPath(root, 'apps', 'desktop', 'resources', 'bin'));
  return roots;
}

/** First existing daemon in the candidate layout. */
export function resolveCompanion({ root, platform = process.platform, exists = existsSync }) {
  for (const dir of candidateRoots({ root, platform })) {
    for (const name of fileNames(platform)) {
      const candidate = joiner(platform)(dir, name);
      if (exists(candidate)) return candidate;
    }
  }
  return null;
}

/** A daemon older than the crate sources must be rebuilt. */
export function isStale({ binaryMtimeMs, sourceMtimeMs }) {
  return Number(binaryMtimeMs ?? 0) < Number(sourceMtimeMs ?? 0);
}

function sourceMtimeMs({ root, platform, mtimeMs }) {
  const joinPath = joiner(platform);
  let newest = 0;
  for (const anchor of SOURCE_ANCHORS) {
    try {
      newest = Math.max(newest, Number(mtimeMs(joinPath(root, anchor)) ?? 0));
    } catch {
      // A missing anchor simply does not contribute to the staleness check.
    }
  }
  return newest;
}

/** Build output path for the current platform (plain profile, no --target). */
function builtCompanionPath({ root, platform = process.platform }) {
  return joiner(platform)(root, 'target', 'debug', fileNames(platform)[0]);
}

/**
 * Ensure a usable daemon exists.
 *
 * Returns `{ ok, built, skipped }`. `ok` reflects whether a daemon is usable
 * after the call; it never throws, so `bun run dev` can continue and report the
 * problem through the app instead of failing the shell chain.
 */
export function ensureVoiceCompanion({
  root,
  platform = process.platform,
  exists = existsSync,
  mtimeMs = (path) => statSync(path).mtimeMs,
  spawn = spawnSync,
  log = console.log,
  env = process.env,
  check = false,
} = {}) {
  if (env?.[SKIP_ENV] === '1') {
    log(`[voice-companion] skipped (${SKIP_ENV}=1)`);
    return { ok: true, built: false, skipped: true };
  }

  const resolved = resolveCompanion({ root, platform, exists });
  const sources = sourceMtimeMs({ root, platform, mtimeMs });
  const stale = resolved ? isStale({ binaryMtimeMs: mtimeMs(resolved), sourceMtimeMs: sources }) : true;

  if (!stale) {
    log(`[voice-companion] ready: ${resolved}`);
    return { ok: true, built: false, skipped: false };
  }

  if (check) {
    log(`[voice-companion] not ready (${resolved ? 'stale' : 'missing'}); run bun run ensure:voice-companion`);
    return { ok: false, built: false, skipped: false };
  }

  log(`[voice-companion] building ${resolved ? 'stale daemon' : 'missing daemon'}...`);
  let status = null;
  try {
    const result = spawn(
      'cargo',
      ['build', '-p', 'voice-companion', '--bin', 'voice-companion', '--no-default-features'],
      { cwd: root, stdio: 'inherit', windowsHide: true },
    );
    status = result?.status ?? null;
  } catch (error) {
    log(`[voice-companion] warning: cargo build failed to start: ${error instanceof Error ? error.message : error}`);
    return { ok: false, built: false, skipped: false };
  }

  if (status !== 0) {
    log('[voice-companion] warning: cargo build failed; voice stays unavailable until the component builds');
    return { ok: false, built: false, skipped: false };
  }

  const expected = builtCompanionPath({ root, platform });
  if (!exists(expected)) {
    log(`[voice-companion] warning: build finished but ${expected} is missing`);
  }
  log(`[voice-companion] ready: ${expected}`);
  return { ok: true, built: true, skipped: false };
}

function main() {
  const check = process.argv.includes('--check');
  const result = ensureVoiceCompanion({
    root: ROOT,
    platform: process.platform,
    exists: existsSync,
    mtimeMs: (path) => statSync(path).mtimeMs,
    spawn: (command, args, options) => spawnSync(command, args, options),
    log: console.log,
    env: process.env,
    check,
  });
  if (check) process.exit(result.ok ? 0 : 1);
}

if (import.meta.main) main();
