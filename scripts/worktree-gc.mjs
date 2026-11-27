#!/usr/bin/env bun
/**
 * worktree-gc -- safe inspection, fail-closed pruning, and warm-lane syncing
 * for Git worktrees on Windows/macOS/Linux.
 *
 * Why this exists:
 *   1. Directory names can be stale/misleading (a dir named `fix-windows-hotpatch`
 *      may currently check out an unmerged PR branch). Always inspect Git status
 *      and `origin/main..HEAD` instead of trusting folder names.
 *   2. On Windows, `git worktree remove` frequently fails mid-way with
 *      `Directory not empty` on deep `node_modules` trees or junctions, leaving
 *      half-deleted directories (`ORPHAN`) behind. Emptying ignored heavy subtrees
 *      via `robocopy /MIR /XJ` before `git worktree remove` prevents zombie dirs
 *      without following junctions.
 *
 * Usage:
 *   bun run worktree:inspect
 *   bun run worktree:prune -- --merged [--delete-branch] [--dry-run]
 *   bun run worktree:prune -- --remove <name|path> [--include-orphans] [--delete-branch]
 *   bun run worktree:prune -- --sync-warm [name...]
 */
import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { assertNoActiveDevSession } from './dev-session-lock.mjs';
import {
  dirSizeGB,
  findLiveProcessesUnder,
  fmtGB,
  rmDirSafe,
  snapshotRunningProcesses,
  tempSearchRoots,
} from './lib/build-fs.mjs';

const DEFAULT_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const WORKTREE_CONTAINERS = ['.agents/worktrees', '.worktrees', '.claude/worktrees'];
const HEAVY_WORKTREE_SUBDIRS = [
  'build.noindex',
  'target',
  join('flowy18_wip', 'build.noindex'),
  join('flowy18_wip', 'target'),
  'node_modules',
  join('ui', 'node_modules'),
];
const TAG = '[worktree-gc]';

function runGit(cwd, gitArgs) {
  return spawnSync('git', gitArgs, {
    cwd,
    encoding: 'utf8',
    windowsHide: true,
  });
}

function resolveBaseRef(repoRoot, explicitBase) {
  if (explicitBase) return explicitBase;
  for (const candidate of ['origin/main', 'main', 'HEAD']) {
    const res = runGit(repoRoot, ['rev-parse', '--verify', '--quiet', candidate]);
    if (res.status === 0) return candidate;
  }
  return 'HEAD';
}

function resolveMainRepoRoot(anyRoot) {
  const res = runGit(anyRoot, ['rev-parse', '--path-format=absolute', '--git-common-dir']);
  if (res.status === 0 && res.stdout.trim()) {
    return resolve(dirname(res.stdout.trim()));
  }
  return null;
}

/** Parse `git worktree list --porcelain` records cleanly. */
export function parsePorcelainWorktrees(output) {
  const entries = [];
  let current = null;
  for (const rawLine of output.split(/\r?\n/)) {
    const line = rawLine.trimEnd();
    if (!line) {
      if (current) entries.push(current);
      current = null;
      continue;
    }
    if (line.startsWith('worktree ')) {
      current = {
        path: resolve(line.slice('worktree '.length).trim()),
        head: null,
        branch: null,
        detached: false,
        locked: false,
        prunable: false,
      };
    } else if (current) {
      if (line.startsWith('HEAD ')) current.head = line.slice('HEAD '.length).trim();
      else if (line.startsWith('branch ')) current.branch = line.slice('branch '.length).replace(/^refs\/heads\//, '').trim();
      else if (line === 'detached') current.detached = true;
      else if (line === 'locked' || line.startsWith('locked ')) current.locked = true;
      else if (line === 'prunable' || line.startsWith('prunable ')) current.prunable = true;
    }
  }
  if (current) entries.push(current);
  return entries;
}

function measureWorktreeHeavyBytes(wtPath) {
  let totalGB = 0;
  const breakdown = {};
  for (const rel of HEAVY_WORKTREE_SUBDIRS) {
    const sub = join(wtPath, rel);
    if (!existsSync(sub)) continue;
    const gb = dirSizeGB(sub);
    totalGB += gb;
    if (gb >= 0.05) breakdown[rel.replace(/\\/g, '/')] = Number(gb.toFixed(2));
  }
  return { totalGB, breakdown };
}

function isDirBlockedByProcessOrLock(wtPath, procSnapshot) {
  for (const lockDir of [join(wtPath, 'build.noindex'), join(wtPath, 'flowy18_wip', 'build.noindex')]) {
    try {
      assertNoActiveDevSession(lockDir);
    } catch (err) {
      return err.message;
    }
  }
  const live = findLiveProcessesUnder([wtPath], procSnapshot);
  if (live.length > 0) {
    return `live process: ${live.map((p) => `${p.name}(${p.pid})`).slice(0, 3).join(', ')}`;
  }
  return null;
}

/**
 * Read-only inventory of all registered non-main worktrees and any unregistered
 * leftover directories under standard worktree container folders.
 */
export function inspectWorktrees(
  repoRoot = DEFAULT_ROOT,
  { baseRef, includeTemp = true, procSnapshot = snapshotRunningProcesses() } = {},
) {
  const invokerRoot = resolve(repoRoot);
  const resolvedBase = resolveBaseRef(invokerRoot, baseRef);
  const listed = runGit(invokerRoot, ['worktree', 'list', '--porcelain']);
  if (listed.status !== 0) {
    throw new Error(`git worktree list failed: ${listed.stderr || listed.stdout}`);
  }

  const rawEntries = parsePorcelainWorktrees(listed.stdout);
  const mainRoot = resolveMainRepoRoot(invokerRoot) ?? rawEntries[0]?.path ?? invokerRoot;
  const registeredPaths = new Set(rawEntries.map((e) => e.path.toLowerCase()));
  const worktrees = [];

  for (let idx = 0; idx < rawEntries.length; idx++) {
    const entry = rawEntries[idx];
    const isMain = idx === 0 || entry.path.toLowerCase() === mainRoot.toLowerCase();
    if (isMain) continue;
    const isCurrent = entry.path.toLowerCase() === invokerRoot.toLowerCase();
    const exists = existsSync(entry.path);
    let dirty = false;
    let unmergedCommits = null;
    let activeReason = null;

    if (exists) {
      const st = runGit(entry.path, ['status', '--porcelain']);
      dirty = st.status !== 0 || st.stdout.trim().length > 0;

      const rev = runGit(entry.path, ['rev-list', '--count', `${resolvedBase}..HEAD`]);
      if (rev.status === 0) {
        const parsed = Number.parseInt(rev.stdout.trim(), 10);
        unmergedCommits = Number.isFinite(parsed) ? parsed : null;
      }
      activeReason = isDirBlockedByProcessOrLock(entry.path, procSnapshot);
    }

    const { totalGB, breakdown } = exists ? measureWorktreeHeavyBytes(entry.path) : { totalGB: 0, breakdown: {} };
    let status;
    if (!exists || entry.prunable) status = 'PRUNABLE (missing)';
    else if (isCurrent) status = 'KEEP (current-worktree)';
    else if (activeReason) status = `KEEP (${activeReason})`;
    else if (entry.locked) status = 'KEEP (locked)';
    else if (dirty) status = 'KEEP (dirty)';
    else if (unmergedCommits === null || unmergedCommits > 0) status = `KEEP (unmerged:${unmergedCommits ?? '?'})`;
    else if (entry.detached) status = 'WARM (detached)';
    else status = 'SAFE TO PRUNE';

    worktrees.push({
      ...entry,
      isMain,
      isCurrent,
      name: basename(entry.path),
      exists,
      dirty,
      unmergedCommits,
      heavyGB: totalGB,
      breakdown,
      status,
    });
  }

  const orphans = [];
  for (const container of WORKTREE_CONTAINERS) {
    const dir = join(mainRoot, container);
    if (!existsSync(dir)) continue;
    let ents = [];
    try {
      ents = readdirSync(dir, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const ent of ents) {
      if (!ent.isDirectory()) continue;
      const full = resolve(join(dir, ent.name));
      if (registeredPaths.has(full.toLowerCase())) continue;
      const { totalGB, breakdown } = measureWorktreeHeavyBytes(full);
      const activeReason = isDirBlockedByProcessOrLock(full, procSnapshot);
      orphans.push({
        name: ent.name,
        container,
        path: full,
        heavyGB: totalGB,
        breakdown,
        activeReason,
        status: activeReason ? `ORPHAN (${activeReason})` : 'ORPHAN (unregistered)',
      });
    }
  }

  const tempCaches = [];
  if (includeTemp) {
    for (const tRoot of tempSearchRoots()) {
      let ents = [];
      try {
        ents = readdirSync(tRoot, { withFileTypes: true });
      } catch {
        continue;
      }
      for (const ent of ents) {
        if (!ent.isDirectory() || !/^flowy18-.*-build$/i.test(ent.name)) continue;
        const p = join(tRoot, ent.name);
        tempCaches.push({ name: ent.name, path: p, sizeGB: dirSizeGB(p) });
      }
    }
  }

  return {
    repoRoot: mainRoot,
    invokerRoot,
    baseRef: resolvedBase,
    worktrees,
    orphans,
    tempCaches,
    procSnapshot,
  };
}

function matchesSelector(target, selectors) {
  if (!selectors || selectors.length === 0) return false;
  const targetPath = resolve(target.path).toLowerCase();
  const targetName = target.name.toLowerCase();
  return selectors.some((sel) => {
    const norm = sel.toLowerCase();
    return targetName === norm || resolve(sel).toLowerCase() === targetPath;
  });
}

function removeWorktreeSafely(repoRoot, wt, { deleteBranch = false, dryRun = false, procSnapshot = [], log = console.log } = {}) {
  if (wt.isMain || resolve(wt.path).toLowerCase() === resolve(repoRoot).toLowerCase()) {
    return { ok: false, path: wt.path, reason: 'main repository cannot be pruned' };
  }
  if (wt.isCurrent) {
    return { ok: false, path: wt.path, reason: 'cannot prune the current working tree' };
  }
  if (wt.dirty) {
    return { ok: false, path: wt.path, reason: 'dirty working tree' };
  }
  if (wt.unmergedCommits === null || wt.unmergedCommits > 0) {
    return { ok: false, path: wt.path, reason: `unmerged commits (${wt.unmergedCommits ?? 'unknown'})` };
  }
  if (wt.locked) {
    return { ok: false, path: wt.path, reason: 'worktree is git-locked' };
  }
  const activeReason = isDirBlockedByProcessOrLock(wt.path, procSnapshot);
  if (activeReason) {
    return { ok: false, path: wt.path, reason: activeReason };
  }

  if (dryRun) {
    log(`${TAG} [dry-run] would remove worktree ${wt.name} (${wt.branch || 'detached'}, ~${fmtGB(wt.heavyGB)})`);
    return { ok: true, dryRun: true, path: wt.path };
  }

  // Clear heavy ignored subtrees (especially Windows `node_modules` & `build.noindex`)
  // before `git worktree remove` so Git never fails halfway with `Directory not empty`.
  for (const rel of HEAVY_WORKTREE_SUBDIRS) {
    const sub = join(wt.path, rel);
    if (existsSync(sub)) {
      rmDirSafe(sub, { label: `${wt.name}/${rel.replace(/\\/g, '/')}`, log: (m) => log(`${TAG}${m}`) });
    }
  }

  const rm = runGit(repoRoot, ['worktree', 'remove', wt.path]);
  if (rm.status !== 0 && existsSync(wt.path)) {
    const leftover = runGit(wt.path, ['status', '--porcelain']);
    if (leftover.status === 0 && leftover.stdout.trim().length > 0) {
      return { ok: false, path: wt.path, reason: `git worktree remove failed: ${rm.stderr.trim()}` };
    }
    rmDirSafe(wt.path, { label: `${wt.name} (leftover)`, log: (m) => log(`${TAG}${m}`) });
  }

  runGit(repoRoot, ['worktree', 'prune']);

  let branchDeleted = false;
  if (deleteBranch && wt.branch) {
    const br = runGit(repoRoot, ['branch', '-d', wt.branch]);
    branchDeleted = br.status === 0;
    if (!branchDeleted) {
      log(`${TAG} WARN: kept local branch '${wt.branch}' (${(br.stderr || br.stdout).trim()})`);
    }
  }

  log(`${TAG} removed worktree ${wt.name}${branchDeleted ? ` and deleted branch '${wt.branch}'` : ''}`);
  return { ok: true, path: wt.path, branchDeleted };
}

/**
 * Prune selected clean/merged worktrees, optionally delete orphan directories or
 * advance clean detached warm worktrees to `baseRef`. Strictly fail-closed.
 */
export function pruneWorktrees(repoRoot = DEFAULT_ROOT, options = {}) {
  const {
    baseRef,
    merged = false,
    remove = [],
    includeOrphans = false,
    syncWarm = [],
    syncAllWarm = false,
    deleteBranch = false,
    dryRun = false,
    log = console.log,
  } = options;

  const report = inspectWorktrees(repoRoot, { baseRef, includeTemp: false });
  const results = { removed: [], rejected: [], synced: [], orphansRemoved: [] };

  const selectedWorktrees = report.worktrees.filter((wt) => {
    if (matchesSelector(wt, remove)) return true;
    if (merged && !wt.isMain && !wt.isCurrent && !wt.detached && wt.branch && !wt.dirty && wt.unmergedCommits === 0 && !wt.locked) return true;
    return false;
  });

  for (const wt of selectedWorktrees) {
    const res = removeWorktreeSafely(report.repoRoot, wt, {
      deleteBranch,
      dryRun,
      procSnapshot: report.procSnapshot,
      log,
    });
    if (res.ok) results.removed.push(res);
    else {
      log(`${TAG} REFUSED ${wt.name}: ${res.reason}`);
      results.rejected.push(res);
    }
  }

  if (includeOrphans && remove.length > 0) {
    for (const orphan of report.orphans) {
      if (!matchesSelector(orphan, remove)) continue;
      if (orphan.activeReason) {
        log(`${TAG} REFUSED orphan ${orphan.name}: ${orphan.activeReason}`);
        results.rejected.push({ ok: false, path: orphan.path, reason: orphan.activeReason });
        continue;
      }
      if (dryRun) {
        log(`${TAG} [dry-run] would remove orphan ${orphan.container}/${orphan.name}`);
        results.orphansRemoved.push({ ok: true, dryRun: true, path: orphan.path });
        continue;
      }
      const res = rmDirSafe(orphan.path, { label: `${orphan.container}/${orphan.name}`, log: (m) => log(`${TAG}${m}`) });
      if (res.removed) results.orphansRemoved.push({ ok: true, path: orphan.path });
      else results.rejected.push({ ok: false, path: orphan.path, reason: res.error || 'failed to remove orphan' });
    }
  }

  if (syncAllWarm || syncWarm.length > 0) {
    for (const wt of report.worktrees) {
      const wanted = syncAllWarm
        ? wt.detached && !wt.dirty && wt.unmergedCommits === 0
        : matchesSelector(wt, syncWarm);
      if (!wanted) continue;
      if (!wt.detached || wt.dirty || wt.unmergedCommits !== 0) {
        const reason = !wt.detached
          ? `checked out on branch '${wt.branch}'`
          : wt.dirty
            ? 'dirty working tree'
            : `unmerged commits (${wt.unmergedCommits ?? '?'})`;
        log(`${TAG} REFUSED sync-warm ${wt.name}: ${reason}`);
        results.rejected.push({ ok: false, path: wt.path, reason });
        continue;
      }
      if (dryRun) {
        log(`${TAG} [dry-run] would detach ${wt.name} at ${report.baseRef}`);
        results.synced.push({ ok: true, dryRun: true, path: wt.path });
        continue;
      }
      const co = runGit(wt.path, ['checkout', '--detach', report.baseRef]);
      if (co.status === 0) {
        log(`${TAG} synced warm worktree ${wt.name} -> ${report.baseRef}`);
        results.synced.push({ ok: true, path: wt.path });
      } else {
        const reason = `checkout failed: ${(co.stderr || co.stdout).trim()}`;
        log(`${TAG} FAILED sync-warm ${wt.name}: ${reason}`);
        results.rejected.push({ ok: false, path: wt.path, reason });
      }
    }
  }

  return { report, results };
}

function printReport(report) {
  console.log(`${TAG} base=${report.baseRef}, repo=${report.repoRoot}`);
  if (report.worktrees.length === 0) {
    console.log(`${TAG} no secondary git worktrees registered.`);
  } else {
    for (const wt of report.worktrees) {
      const refLabel = wt.detached ? 'detached' : wt.branch || 'unknown';
      const detail = Object.entries(wt.breakdown)
        .map(([k, v]) => `${k}=${v}G`)
        .join(', ');
      console.log(
        `  [${wt.status}] ${wt.name} (${refLabel} @ ${(wt.head || '').slice(0, 9)})` +
          ` | heavy=${fmtGB(wt.heavyGB)}${detail ? ` (${detail})` : ''}`,
      );
    }
  }

  if (report.orphans.length > 0) {
    console.log(`${TAG} unregistered orphan directories:`);
    for (const o of report.orphans) {
      console.log(`  [${o.status}] ${o.container}/${o.name} | heavy=${fmtGB(o.heavyGB)}`);
    }
  }

  if (report.tempCaches.length > 0) {
    console.log(`${TAG} isolated Temp build caches:`);
    for (const t of report.tempCaches) {
      console.log(`  ${t.name}: ${fmtGB(t.sizeGB)} (${t.path})`);
    }
  }
}

const isEntrypoint = Boolean(
  process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url),
);

if (isEntrypoint) {
  const args = process.argv.slice(2);
  const readMulti = (flag) => {
    const out = [];
    for (let i = 0; i < args.length; i++) {
      if (args[i] === flag && args[i + 1] && !args[i + 1].startsWith('--')) {
        out.push(args[++i]);
      } else if (args[i].startsWith(`${flag}=`)) {
        out.push(args[i].slice(flag.length + 1));
      }
    }
    return out;
  };

  const repoIdx = args.indexOf('--repo');
  const repoRoot = repoIdx >= 0 && args[repoIdx + 1] ? resolve(args[repoIdx + 1]) : DEFAULT_ROOT;
  const baseIdx = args.indexOf('--base');
  const baseRef = baseIdx >= 0 && args[baseIdx + 1] ? args[baseIdx + 1] : undefined;

  const isPrune = args.includes('--prune');
  const isInspect = args.includes('--inspect') || !isPrune;
  const merged = args.includes('--merged');
  const includeOrphans = args.includes('--include-orphans');
  const deleteBranch = args.includes('--delete-branch');
  const dryRun = args.includes('--dry-run');
  const remove = readMulti('--remove');
  const syncWarm = readMulti('--sync-warm');
  const syncAllWarm = args.includes('--sync-warm') && syncWarm.length === 0;

  if (isInspect && !isPrune) {
    const report = inspectWorktrees(repoRoot, { baseRef });
    printReport(report);
    process.exit(0);
  }

  if (!merged && remove.length === 0 && !syncAllWarm && syncWarm.length === 0) {
    const report = inspectWorktrees(repoRoot, { baseRef });
    printReport(report);
    console.log(`${TAG} no mutation requested; pass --merged, --remove <name>, or --sync-warm [name].`);
    process.exit(0);
  }

  const { results } = pruneWorktrees(repoRoot, {
    baseRef,
    merged,
    remove,
    includeOrphans,
    syncWarm,
    syncAllWarm,
    deleteBranch,
    dryRun,
  });

  if (results.rejected.length > 0) {
    process.exit(1);
  }
  process.exit(0);
}
