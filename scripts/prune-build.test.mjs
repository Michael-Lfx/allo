import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

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
});
