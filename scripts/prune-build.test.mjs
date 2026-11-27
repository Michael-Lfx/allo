import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, utimesSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, describe, expect, test } from 'bun:test';
import { createDevSessionLock } from './dev-session-lock.mjs';
import { isProtectedBuildPath } from './lib/build-fs.mjs';
import {
  discoverCheckoutBuildTargets,
  guardBuildTarget,
  hasMatchingEnvBuildDir,
  isFlowy18TempBuildName,
  pruneBuildDirGenerations,
  shouldPruneTempEntry,
} from './prune-build.mjs';

const REPO_ROOT = fileURLToPath(new URL('..', import.meta.url));
const tempDirs = [];
afterEach(() => {
  for (const dir of tempDirs.splice(0)) rmSync(dir, { recursive: true, force: true });
});

function writeSizedFile(filePath, sizeBytes, ageSeconds = 0) {
  mkdirSync(dirname(filePath), { recursive: true });
  writeFileSync(filePath, Buffer.alloc(sizeBytes, 0x61));
  if (ageSeconds > 0) {
    const t = new Date(Date.now() - ageSeconds * 1000);
    utimesSync(filePath, t, t);
  }
}

const rootPackage = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'));
const pruneSource = readFileSync(new URL('./prune-build.mjs', import.meta.url), 'utf8');
const devConfig = readFileSync(new URL('../apps/desktop/tauri.dev.conf.json', import.meta.url), 'utf8');

describe('build cache entry points', () => {
  test('default development and test commands do not invoke prune-build', () => {
    for (const key of ['dev', 'dev:web', 'serve:web', 'test', 'test:fast']) {
      expect(rootPackage.scripts[key]).not.toContain('prune-build');
    }
    expect(devConfig).not.toContain('prune-build');
  });

  test('prune-build is read-only by default and has explicit gc/clean modes', () => {
    expect(pruneSource).toContain("const isInspect = args.includes('--inspect');");
    expect(pruneSource).toContain("const isGc = args.includes('--gc');");
    expect(pruneSource).toContain("const isClean = args.includes('--clean') || isRelease;");
    expect(pruneSource).toContain('inspectBuildCache();');
    expect(pruneSource).toContain('assertNoActiveDevSession(BUILD_DIR);');
  });

  test('new build:inspect:all / build:gc:all scripts stay guarded and skip --prune-temp by default', () => {
    expect(rootPackage.scripts['build:inspect:all']).toContain('--inspect');
    expect(rootPackage.scripts['build:inspect:all']).toContain('--flowy18');
    expect(rootPackage.scripts['build:inspect:all']).toContain('--worktrees');
    expect(rootPackage.scripts['build:gc:all']).toContain('--gc');
    expect(rootPackage.scripts['build:gc:all']).not.toContain('--prune-temp');
  });
});

describe('build cache cap enforcement', () => {
  test('the cap is enforced through cargo-sweep and says so when it is missing', () => {
    // `build:gc` can only bound the cache with cargo-sweep; the incremental-only
    // fallback never touches deps/, which is where a bloated cache lives.
    expect(pruneSource).toContain('cargo install cargo-sweep --locked');
    expect(pruneSource).toContain('cargo-sweep is not installed');
    expect(pruneSource).toContain('deps/ (the bulk) is left untouched');
    expect(pruneSource).toContain('cargo-sweep MISSING');
  });

  test('the sweep is allowed to finish instead of dying on a short timeout', () => {
    // A 160G+ sweep needs minutes; a 60s timeout killed it mid-delete and the
    // run still exited 0, leaving the cache half-trimmed and unexplained.
    expect(pruneSource).toContain('CARGO_SWEEP_TIMEOUT_MS');
    expect(pruneSource).not.toContain('timeout: 60_000');
    expect(pruneSource).toContain('the sweep was interrupted');
  });

  test('--cap drives the sweep cap, not just the warning threshold', () => {
    // `--cap 80` used to change only the residual warning threshold while the
    // sweep kept trimming to the hard-coded constant, so the flag looked
    // accepted and did nothing. One resolved value must feed both.
    expect(pruneSource).toContain('const BUILD_CAP_GB = capIsValid ? capValue : BUILD_MAXSIZE_GB;');
    expect(pruneSource).toContain("cargoSweepMaxsize(BUILD_DIR, BUILD_CAP_GB, 'build.noindex')");
    expect(pruneSource).not.toContain('const CAP_GB');
    expect(pruneSource).toContain('--cap needs a positive number of GB');
    expect(pruneSource).toContain('manifestRoot = ROOT');
  });
});

describe('flowy18, worktrees, and temp build safety guards', () => {
  test('recognizes flowy18 temp build directory names without matching unrelated folders', () => {
    expect(isFlowy18TempBuildName('flowy18-main-build')).toBe(true);
    expect(isFlowy18TempBuildName('flowy18-build-dir')).toBe(true);
    expect(isFlowy18TempBuildName('flowy18-router-build')).toBe(true);
    expect(isFlowy18TempBuildName('nomi-empty-123')).toBe(false);
    expect(isFlowy18TempBuildName('flowy-notes')).toBe(false);
  });

  test('blocks cleanup when CARGO_BUILD_BUILD_DIR, CARGO_TARGET_DIR, or active binary matches target', () => {
    const targetDir = join('D:', 'tmp', 'flowy18-main-build');
    expect(
      hasMatchingEnvBuildDir([targetDir], { CARGO_BUILD_BUILD_DIR: join(targetDir, 'nested') }),
    ).toBe(true);
    expect(
      guardBuildTarget(
        { buildDir: targetDir, manifestRoot: join('D:', 'repo', 'flowy18_wip') },
        { env: { CARGO_BUILD_BUILD_DIR: targetDir } },
      ).blocked,
    ).toBe(true);
    expect(
      guardBuildTarget(
        { buildDir: targetDir, manifestRoot: join('D:', 'repo', 'flowy18_wip') },
        { env: {}, activePaths: [join(targetDir, 'debug', 'flowy-app.exe')] },
      ).blocked,
    ).toBe(true);
    expect(
      guardBuildTarget(
        { buildDir: targetDir, manifestRoot: join('D:', 'repo', 'flowy18_wip') },
        { env: {}, activePaths: [join('D:', 'other', 'flowy-app.exe')] },
      ).blocked,
    ).toBe(false);
  });

  test('temp pruning enforces age threshold and guard status', () => {
    expect(shouldPruneTempEntry({ ageDays: 6.9 }, 7, { blocked: false }).prune).toBe(false);
    expect(shouldPruneTempEntry({ ageDays: 10 }, 7, { blocked: true, reason: 'active' }).prune).toBe(false);
    expect(shouldPruneTempEntry({ ageDays: 7.1 }, 7, { blocked: false }).prune).toBe(true);
  });

  test('discovers flowy18_wip and sibling worktree targets with proper manifestRoot', () => {
    const sandbox = mkdtempSync(join(tmpdir(), 'prune-build-test-'));
    try {
      const flowyMain = join(sandbox, 'flowy18_wip');
      mkdirSync(join(flowyMain, 'build.noindex'), { recursive: true });
      const wtFlowy = join(sandbox, '.agents', 'worktrees', 'wt-alpha', 'flowy18_wip');
      mkdirSync(join(wtFlowy, 'target'), { recursive: true });
      const wtBeta = join(sandbox, '.agents', 'worktrees', 'wt-beta');
      mkdirSync(join(wtBeta, 'flowy18_wip'), { recursive: true });

      const foundFromMain = discoverCheckoutBuildTargets(sandbox, { flowy18: true, worktrees: true });
      expect(foundFromMain.map((t) => [t.label, t.manifestRoot])).toEqual([
        ['flowy18_wip', flowyMain],
        ['worktree/wt-alpha/flowy18_wip', wtFlowy],
      ]);

      const foundFromSibling = discoverCheckoutBuildTargets(wtBeta, { flowy18: true, worktrees: true });
      expect(foundFromSibling.map((t) => [t.label, t.manifestRoot])).toEqual([
        ['main/flowy18_wip', flowyMain],
        ['worktree/wt-alpha/flowy18_wip', wtFlowy],
      ]);
    } finally {
      rmSync(sandbox, { recursive: true, force: true });
    }
  });

  test('pruneBuildDirGenerations evicts older duplicate crate hashes while preserving the newest generation per crate', () => {
    const profileDir = mkdtempSync(join(tmpdir(), 'flowy-builddir-gc-'));
    tempDirs.push(profileDir);

    writeSizedFile(join(profileDir, 'build', 'flowy-studio', 'oldhash111', 'out', 'lib.rmeta'), 2048, 7200);
    writeSizedFile(join(profileDir, 'build', 'flowy-studio', 'newhash222', 'out', 'lib.rmeta'), 1024, 10);
    writeSizedFile(join(profileDir, 'build', 'windows', 'onlyhash999', 'out', 'lib.rmeta'), 1024, 172_800);
    writeSizedFile(join(profileDir, 'incremental', 'flowy_studio-oldaaa', 's-1-a', 'dep-graph.bin'), 2048, 7200);
    writeSizedFile(join(profileDir, 'incremental', 'flowy_studio-newbbb', 's-2-b', 'dep-graph.bin'), 1024, 5);

    const res = pruneBuildDirGenerations(profileDir, 3072);
    expect(res.scanned).toBe(true);
    expect(res.removedCount).toBe(2);
    expect(existsSync(join(profileDir, 'build', 'flowy-studio', 'oldhash111'))).toBe(false);
    expect(existsSync(join(profileDir, 'build', 'flowy-studio', 'newhash222'))).toBe(true);
    expect(existsSync(join(profileDir, 'build', 'windows', 'onlyhash999'))).toBe(true);
    expect(existsSync(join(profileDir, 'incremental', 'flowy_studio-oldaaa'))).toBe(false);
    expect(existsSync(join(profileDir, 'incremental', 'flowy_studio-newbbb'))).toBe(true);
  });

  test('protects dx-capture-wrapper/target and skips --build-dir owned by active dev session', () => {
    expect(isProtectedBuildPath(join('flowy18_wip', 'scripts', 'dx-capture-wrapper', 'target'))).toBe(true);
    expect(isProtectedBuildPath(join('flowy18_wip', 'build.noindex'))).toBe(false);

    const customBuild = mkdtempSync(join(tmpdir(), 'flowy-active-lock-'));
    tempDirs.push(customBuild);
    const lock = createDevSessionLock({
      buildDir: customBuild,
      workspaceRoot: 'ws',
      targetDir: 'target',
    });
    try {
      writeSizedFile(join(customBuild, 'debug', 'build', 'demo', 'oldh', 'out', 'a.bin'), 2048, 3600);
      writeSizedFile(join(customBuild, 'debug', 'build', 'demo', 'newh', 'out', 'a.bin'), 1024, 10);
      const out = spawnSync('bun', ['scripts/prune-build.mjs', '--gc', '--build-dir', customBuild, '--cap', '0.000001'], {
        cwd: REPO_ROOT,
        encoding: 'utf8',
      });
      expect(out.status).toBe(0);
      expect(out.stdout).toContain('SKIP [custom:');
      expect(existsSync(join(customBuild, 'debug', 'build', 'demo', 'oldh'))).toBe(true);
    } finally {
      lock.release();
    }
  });
});
