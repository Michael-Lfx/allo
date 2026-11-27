import { execSync, spawnSync } from 'node:child_process';
import {
  existsSync,
  lstatSync,
  mkdtempSync,
  readdirSync,
  rmSync,
  statSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join, normalize, resolve } from 'node:path';

export const isWin = process.platform === 'win32';
const PROTECTED_TARGET_RE = /[\\/]scripts[\\/]dx-capture-wrapper[\\/]target(?:[\\/]|$)/i;

/** Return true for protected local tooling caches (for example dx hot-patch wrapper). */
export function isProtectedBuildPath(targetPath) {
  return PROTECTED_TARGET_RE.test(normalize(resolve(targetPath)));
}

/**
 * Sum file sizes (and optionally track newest file/entry mtime) in one iterative walk.
 * Uses Dirent.isDirectory(), which returns false for symlinks/junctions so walks never loop.
 */
export function dirStats(dir) {
  let bytes = 0;
  let maxMtimeMs = 0;
  try {
    const rootStat = lstatSync(dir);
    if (rootStat.isSymbolicLink()) return { bytes: 0, maxMtimeMs: rootStat.mtimeMs };
    maxMtimeMs = rootStat.mtimeMs;
  } catch {
    return { bytes: 0, maxMtimeMs: 0 };
  }

  const stack = [dir];
  while (stack.length) {
    const current = stack.pop();
    let entries;
    try {
      entries = readdirSync(current, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const ent of entries) {
      const fullPath = join(current, ent.name);
      if (ent.isDirectory()) {
        stack.push(fullPath);
        continue;
      }
      try {
        const st = statSync(fullPath);
        bytes += st.size;
        if (st.mtimeMs > maxMtimeMs) maxMtimeMs = st.mtimeMs;
      } catch {
        /* entry vanished or locked */
      }
    }
  }
  return { bytes, maxMtimeMs };
}

/** Sum file sizes under a directory using a pure-Node walk. */
export function dirSizeBytes(dir) {
  return dirStats(dir).bytes;
}

/** Logical directory size in GiB (consistent across Windows, macOS, Linux). */
export function dirSizeGB(dir) {
  if (!existsSync(dir)) return 0;
  try {
    return dirSizeBytes(dir) / 1024 ** 3;
  } catch {
    return 0;
  }
}

/** Format GiB cleanly for CLI status lines. */
export function fmtGB(gb) {
  if (gb < 1) return `${(gb * 1024).toFixed(0)}M`;
  return `${gb.toFixed(1)}G`;
}

/**
 * Windows: empty-mirror a scratch directory over `dir` with robocopy so deep
 * NTFS trees and long paths (>260 chars) are cleared fast without following junctions (/XJ).
 */
export function robocopyEmptyMirror(dir) {
  if (!isWin) return false;
  let scratch;
  try {
    scratch = mkdtempSync(join(tmpdir(), 'nomi-empty-'));
    const r = spawnSync(
      'robocopy',
      [scratch, dir, '/MIR', '/XJ', '/R:0', '/W:0', '/MT:16', '/NFL', '/NDL', '/NJH', '/NJS', '/NC', '/NS', '/NP'],
      { stdio: 'ignore', timeout: 120_000, windowsHide: true },
    );
    return r.status !== null && r.status < 8;
  } catch {
    return false;
  } finally {
    if (scratch) {
      try {
        rmSync(scratch, { recursive: true, force: true });
      } catch {
        /* best effort */
      }
    }
  }
}

/**
 * Remove a directory tree safely without following symlinks/junctions.
 * Protected tooling paths (such as `dx-capture-wrapper/target`) are always refused.
 */
export function rmDirSafe(dir, { label, log } = {}) {
  if (!dir || !existsSync(dir)) return { removed: false, skipped: false };
  if (isProtectedBuildPath(dir)) {
    log?.(`  skipped protected path ${label || dir}`);
    return { removed: false, skipped: true };
  }
  try {
    if (isWin) robocopyEmptyMirror(dir);
    rmSync(dir, { recursive: true, force: true });
    if (existsSync(dir)) {
      return { removed: false, skipped: false, error: 'directory residue remains after remove' };
    }
    if (label) log?.(`  removed ${label}`);
    return { removed: true, skipped: false };
  } catch (error) {
    log?.(`  WARN: could not remove ${label || dir}: ${error.message}`);
    return { removed: false, skipped: false, error: error.message };
  }
}

/**
 * Capture running process executable paths once per CLI run so multi-worktree /
 * multi-scope guards do not spawn PowerShell/ps repeatedly (each spawn costs ~1s on Windows).
 */
export function snapshotRunningProcesses() {
  try {
    if (isWin) {
      const ps = 'Get-CimInstance Win32_Process -Filter "ExecutablePath IS NOT NULL" -Property ProcessId,Name,ExecutablePath | Select-Object ProcessId,Name,ExecutablePath | ConvertTo-Json -Compress';
      const out = execSync(`powershell -NoProfile -NonInteractive -Command "${ps.replace(/"/g, '\\"')}"`, {
        encoding: 'utf8',
        windowsHide: true,
        timeout: 15_000,
      }).trim();
      if (!out) return [];
      const parsed = JSON.parse(out);
      const rows = Array.isArray(parsed) ? parsed : [parsed];
      return rows.map((r) => ({
        pid: String(r.ProcessId),
        name: r.Name,
        path: r.ExecutablePath,
        lowerPath: (r.ExecutablePath || '').toLowerCase(),
      }));
    }

    const out = execSync('ps -eo pid=,args=', { encoding: 'utf8', timeout: 10_000 });
    const matches = [];
    for (const line of out.split(/\r?\n/)) {
      const m = line.trim().match(/^(\d+)\s+(.+)$/);
      if (!m) continue;
      const [, pid, args] = m;
      matches.push({
        pid,
        name: args.split(/\s+/)[0],
        path: args,
        lowerPath: args.toLowerCase(),
      });
    }
    return matches;
  } catch {
    return [];
  }
}

/**
 * Return matching live executables running from inside any of `dirPrefixes`.
 * Never kills processes; callers use this to fail-closed before touching active caches or worktrees.
 */
export function findLiveProcessesUnder(dirPrefixes, snapshot = snapshotRunningProcesses()) {
  const normalized = dirPrefixes
    .filter(Boolean)
    .map((p) => normalize(resolve(p)).replace(/[\\/]+$/, '').toLowerCase());
  if (normalized.length === 0 || snapshot.length === 0) return [];

  return snapshot.filter((proc) =>
    normalized.some((prefix) => proc.lowerPath === prefix || proc.lowerPath.startsWith(`${prefix}\\`) || proc.lowerPath.startsWith(`${prefix}/`)),
  );
}

/** Candidate temporary directories where Windows/Unix Cargo `build-dir` isolation caches live. */
export function tempSearchRoots() {
  const roots = new Set([resolve(tmpdir())]);
  if (process.env.LOCALAPPDATA) {
    roots.add(resolve(join(process.env.LOCALAPPDATA, 'Temp')));
  }
  return [...roots].filter((p) => existsSync(p));
}
