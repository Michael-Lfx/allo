#!/usr/bin/env bun
/**
 * run-dev — cross-platform launcher for `tauri dev` with NOMI_CHANNEL=dev.
 *
 * Unix-style inline env (`NOMI_CHANNEL=dev tauri dev ...`) fails on Windows
 * PowerShell/CMD. This script sets the env in-process and forwards argv.
 */
import { spawnSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const tauriArgs = [
  'dev',
  '--config',
  'apps/desktop/tauri.conf.json',
  '--config',
  'apps/desktop/tauri.dev.conf.json',
  ...process.argv.slice(2),
];

const result = spawnSync('bun', ['x', 'tauri', ...tauriArgs], {
  cwd: ROOT,
  stdio: 'inherit',
  env: { ...process.env, NOMI_CHANNEL: 'dev' },
});

if (result.error) {
  console.error('[run-dev] failed to start tauri:', result.error.message);
  process.exit(1);
}

process.exit(result.status ?? 1);
