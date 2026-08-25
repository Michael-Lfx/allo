#!/usr/bin/env bun
/**
 * Run `vite build` under Node with an old-space cap that fits the machine.
 *
 * Vite's bin shebang is `#!/usr/bin/env node`, so `bun run` still launches V8.
 * GitHub-hosted Node defaults to ~2GB old-space; this SPA OOMs there during
 * transform (see v0.0.4-linux / v0.0.4-macos). Raising the cap does not pin
 * that much RAM — it only raises the ceiling. macos-14 has ~7GB, so stay at
 * 4096 there; ubuntu/windows runners have ~16GB and can use 8192.
 *
 * `NODE_OPTIONS=--max-old-space-size=N` still wins when already set.
 */
import { spawn } from 'node:child_process';
import { dirname, delimiter, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import os from 'node:os';

const uiRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = join(uiRoot, '..');

function resolveHeapMb() {
  const fromEnv = /(?:^|\s)--max-old-space-size=(\d+)/.exec(process.env.NODE_OPTIONS ?? '');
  if (fromEnv) return Number(fromEnv[1]);
  const totalGb = os.totalmem() / 1024 ** 3;
  return totalGb < 10 ? 4096 : 8192;
}

const heapMb = resolveHeapMb();
const env = { ...process.env };
if (!/(?:^|\s)--max-old-space-size=/.test(env.NODE_OPTIONS ?? '')) {
  env.NODE_OPTIONS = [env.NODE_OPTIONS, `--max-old-space-size=${heapMb}`]
    .filter(Boolean)
    .join(' ');
}

const binDirs = [join(uiRoot, 'node_modules', '.bin'), join(repoRoot, 'node_modules', '.bin')];
env.PATH = [...binDirs, env.PATH ?? env.Path ?? ''].join(delimiter);
if (process.platform === 'win32') {
  env.Path = env.PATH;
}

console.log(`[vite-build] NODE_OPTIONS=${env.NODE_OPTIONS}`);

const child = spawn('vite', ['build', ...process.argv.slice(2)], {
  cwd: uiRoot,
  env,
  stdio: 'inherit',
  shell: true,
});

child.on('error', (error) => {
  console.error('Failed to start vite build:', error.message);
  process.exit(1);
});

child.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 1);
});
