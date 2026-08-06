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
