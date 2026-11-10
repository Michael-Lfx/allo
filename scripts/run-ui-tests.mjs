#!/usr/bin/env node
/**
 * Run the full UI suite in isolated batches.
 *
 * Bun 1.3.14 can segfault when a single `bun test --isolate` process runs the
 * whole renderer suite (the crash moves between files and disappears when the
 * same files run in smaller groups). Keep the isolation semantics, but split
 * the run so one heavy process cannot take the whole suite down.
 */
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const UI = join(ROOT, 'ui');
const GROUPS = [
  ['src/renderer/pages'],
  ['src/renderer/components'],
  ['src/renderer/appProviders.structure.test.ts'],
  ['src/renderer/hooks'],
  ['src/renderer/services'],
  ['src/renderer/utils'],
  ['src/renderer/stores'],
  ['src/renderer/features'],
  ['src/renderer/styles'],
  ['src/common'],
];

for (const group of GROUPS) {
  const result = spawnSync(process.execPath, ['test', '--isolate', ...group], {
    cwd: UI,
    stdio: 'inherit',
    env: process.env,
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

console.log('UI test batches passed');
