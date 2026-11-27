import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { afterEach, describe, expect, test } from 'bun:test';
import { inspectWorktrees, pruneWorktrees } from './worktree-gc.mjs';

const tempDirs = [];
afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

function git(cwd, args) {
  return execFileSync('git', args, { cwd, encoding: 'utf8', windowsHide: true }).trim();
}

function createSandboxRepo() {
  const base = mkdtempSync(join(tmpdir(), 'flowy-wt-test-'));
  tempDirs.push(base);
  const repo = join(base, 'repo');
  mkdirSync(repo, { recursive: true });
  git(repo, ['init', '-b', 'main']);
  git(repo, ['config', 'core.autocrlf', 'false']);
  git(repo, ['config', 'user.name', 'Test User']);
  git(repo, ['config', 'user.email', 'test@example.com']);
  writeFileSync(join(repo, '.gitignore'), 'node_modules/\nbuild.noindex/\ntarget/\n', 'utf8');
  writeFileSync(join(repo, 'README.md'), '# sandbox\n', 'utf8');
  git(repo, ['add', '.gitignore', 'README.md']);
  git(repo, ['commit', '-m', 'chore: init']);
  return { base, repo };
}

describe('worktree-gc fail-closed guarantees', () => {
  test('refuses to prune worktrees with unmerged commits or dirty files', () => {
    const { repo } = createSandboxRepo();
    const unmergedWt = join(repo, '.agents', 'worktrees', 'unmerged-feat');
    const dirtyWt = join(repo, '.agents', 'worktrees', 'dirty-fix');

    git(repo, ['worktree', 'add', '-b', 'feat/unmerged', unmergedWt, 'main']);
    writeFileSync(join(unmergedWt, 'feature.txt'), 'unmerged\n', 'utf8');
    git(unmergedWt, ['add', 'feature.txt']);
    git(unmergedWt, ['commit', '-m', 'feat: unmerged work']);

    git(repo, ['worktree', 'add', '-b', 'fix/dirty', dirtyWt, 'main']);
    writeFileSync(join(dirtyWt, 'scratch.txt'), 'dirty\n', 'utf8');

    const report = inspectWorktrees(repo, { baseRef: 'main', includeTemp: false });
    const unmergedEntry = report.worktrees.find((w) => w.name === 'unmerged-feat');
    const dirtyEntry = report.worktrees.find((w) => w.name === 'dirty-fix');
    expect(unmergedEntry?.status).toBe('KEEP (unmerged:1)');
    expect(dirtyEntry?.status).toBe('KEEP (dirty)');

    const { results } = pruneWorktrees(repo, {
      baseRef: 'main',
      remove: ['unmerged-feat', 'dirty-fix'],
      log: () => {},
    });
    expect(results.removed).toHaveLength(0);
    expect(results.rejected).toHaveLength(2);
    expect(existsSync(unmergedWt)).toBe(true);
    expect(existsSync(dirtyWt)).toBe(true);
  }, 15_000);

  test('safely removes clean merged worktrees even with deep node_modules and deletes merged branch when requested', () => {
    const { repo } = createSandboxRepo();
    const mergedWt = join(repo, '.agents', 'worktrees', 'merged-wt');
    const warmWt = join(repo, '.agents', 'worktrees', 'warm-wt');

    git(repo, ['worktree', 'add', '-b', 'docs/merged', mergedWt, 'main']);
    // Simulate a deep node_modules tree + build cache inside the merged worktree.
    const deepDir = join(mergedWt, 'node_modules', '.bun', 'pkg@1.0.0', 'node_modules', 'pkg', 'dist');
    mkdirSync(deepDir, { recursive: true });
    writeFileSync(join(deepDir, 'index.js'), 'export {};\n', 'utf8');

    // Detached warm worktree should NOT be deleted by `--merged`.
    git(repo, ['worktree', 'add', '--detach', warmWt, 'main']);

    const { results } = pruneWorktrees(repo, {
      baseRef: 'main',
      merged: true,
      deleteBranch: true,
      log: () => {},
    });
    expect(results.removed).toHaveLength(1);
    expect(resolve(results.removed[0].path)).toBe(resolve(mergedWt));
    expect(results.removed[0].branchDeleted).toBe(true);
    expect(existsSync(mergedWt)).toBe(false);
    expect(existsSync(warmWt)).toBe(true);

    const listed = git(repo, ['worktree', 'list']);
    expect(listed).not.toContain('merged-wt');
    expect(listed).toContain('warm-wt');
  }, 15_000);

  test('detects unregistered orphan directories without deleting them unless explicitly requested', () => {
    const { repo } = createSandboxRepo();
    const orphan = join(repo, '.worktrees', 'stale-orphan');
    mkdirSync(join(orphan, 'node_modules'), { recursive: true });
    writeFileSync(join(orphan, 'node_modules', 'junk.txt'), 'junk\n', 'utf8');

    const report = inspectWorktrees(repo, { baseRef: 'main', includeTemp: false });
    expect(report.orphans.some((o) => o.name === 'stale-orphan')).toBe(true);

    // Passing --remove without --include-orphans keeps the orphan intact.
    pruneWorktrees(repo, {
      baseRef: 'main',
      remove: ['stale-orphan'],
      includeOrphans: false,
      log: () => {},
    });
    expect(existsSync(orphan)).toBe(true);

    // Explicit --include-orphans removes it cleanly.
    const { results } = pruneWorktrees(repo, {
      baseRef: 'main',
      remove: ['stale-orphan'],
      includeOrphans: true,
      log: () => {},
    });
    expect(results.orphansRemoved).toHaveLength(1);
    expect(existsSync(orphan)).toBe(false);
  }, 15_000);

  test('CLI --prune exits non-zero when refusing an unmerged worktree', () => {
    const { repo } = createSandboxRepo();
    const wt = join(repo, '.agents', 'worktrees', 'blocked-cli');
    git(repo, ['worktree', 'add', '-b', 'feat/blocked', wt, 'main']);
    writeFileSync(join(wt, 'a.txt'), 'x\n', 'utf8');
    git(wt, ['add', 'a.txt']);
    git(wt, ['commit', '-m', 'feat: blocked']);

    const out = spawnSync(
      'bun',
      ['scripts/worktree-gc.mjs', '--repo', repo, '--base', 'main', '--prune', '--remove', 'blocked-cli'],
      { cwd: join(import.meta.dir, '..'), encoding: 'utf8' },
    );
    expect(out.status).toBe(1);
    expect(out.stdout).toContain('REFUSED blocked-cli');
    expect(existsSync(wt)).toBe(true);
  }, 15_000);

  test('protects main repository and detects orphans when invoked from inside a linked worktree', () => {
    const { repo } = createSandboxRepo();
    const linkedWt = join(repo, '.agents', 'worktrees', 'linked-invoker');
    const mergedWt = join(repo, '.agents', 'worktrees', 'sibling-merged');
    const orphan = join(repo, '.worktrees', 'orphan-from-main');
    git(repo, ['worktree', 'add', '-b', 'feat/invoker', linkedWt, 'main']);
    git(repo, ['worktree', 'add', '-b', 'docs/sibling', mergedWt, 'main']);
    mkdirSync(orphan, { recursive: true });

    const report = inspectWorktrees(linkedWt, { baseRef: 'main', includeTemp: false });
    expect(resolve(report.repoRoot)).toBe(resolve(repo));
    expect(report.worktrees.some((w) => resolve(w.path) === resolve(repo))).toBe(false);
    expect(report.worktrees.find((w) => w.name === 'linked-invoker')?.status).toBe('KEEP (current-worktree)');
    expect(report.worktrees.find((w) => w.name === 'sibling-merged')?.status).toBe('SAFE TO PRUNE');
    expect(report.orphans.some((o) => o.name === 'orphan-from-main')).toBe(true);

    // `--merged` run from inside linkedWt must prune sibling-merged without touching main repo or linkedWt.
    const { results } = pruneWorktrees(linkedWt, {
      baseRef: 'main',
      merged: true,
      log: () => {},
    });
    expect(results.removed).toHaveLength(1);
    expect(resolve(results.removed[0].path)).toBe(resolve(mergedWt));
    expect(existsSync(repo)).toBe(true);
    expect(existsSync(linkedWt)).toBe(true);
    expect(existsSync(mergedWt)).toBe(false);
  }, 15_000);
});
