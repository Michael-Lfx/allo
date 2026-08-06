#!/usr/bin/env bun
/**
 * run-dev — long-lived cross-platform supervisor for Vite + `tauri dev`.
 *
 * Unix-style inline env (`NOMI_CHANNEL=dev tauri dev ...`) fails on Windows
 * PowerShell/CMD. This script sets the env in-process and forwards argv.
 */
import { spawn } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  createRestartControlDirectory,
  createRestartSignal,
  removeRestartControlDirectory,
} from './run-dev-restart-signal.mjs';
import { createSupervisor, waitForExit } from './run-dev-supervisor.mjs';
import { createDevSessionLock } from './dev-session-lock.mjs';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const VITE_ENTRY = join(ROOT, 'ui', 'node_modules', 'vite', 'bin', 'vite.js');
const TAURI_ENTRY = join(ROOT, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');
const tauriArgs = [
  'dev',
  '--config',
  'apps/desktop/tauri.conf.json',
  '--config',
  'apps/desktop/tauri.dev.conf.json',
  ...process.argv.slice(2),
];

const env = { ...process.env, NOMI_CHANNEL: 'dev' };
const devSessionLock = createDevSessionLock({
  buildDir: join(ROOT, 'build.noindex'),
  workspaceRoot: ROOT,
  targetDir: join(ROOT, 'target'),
});
const restartControlDirectory = createRestartControlDirectory();

function waitForChildExit(child, timeoutMs) {
  if (!child || child.exitCode !== null) return Promise.resolve(true);
  return new Promise((resolve) => {
    let settled = false;
    const finish = (value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve(value);
    };
    const timer = setTimeout(() => finish(false), timeoutMs);
    waitForExit(child).then(() => finish(true));
  });
}

async function runTaskkill(args) {
  const command = spawn('taskkill', args, { stdio: 'ignore', windowsHide: true });
  await waitForExit(command);
}

async function stopProcess(child, _reason) {
  if (!child || child.exitCode !== null) return;

  if (process.platform === 'win32' && child.pid) {
    // Give console-aware children a chance to close their windows and run the
    // Tauri cleanup coordinator before escalating to a process-tree kill.
    try { child.kill('SIGINT'); } catch { /* child may already be closing */ }
    if (await waitForChildExit(child, 5_000)) return;
    await runTaskkill(['/PID', String(child.pid), '/T']);
    if (await waitForChildExit(child, 5_000)) return;
    await runTaskkill(['/PID', String(child.pid), '/T', '/F']);
    await waitForChildExit(child, 5_000);
    return;
  }

  try { child.kill('SIGTERM'); } catch { /* child may already be closing */ }
  if (await waitForChildExit(child, 5_000)) return;
  try { child.kill('SIGKILL'); } catch { /* best effort */ }
  await waitForChildExit(child, 5_000);
}

async function waitForVite() {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    try {
      const response = await fetch('http://127.0.0.1:5173/', {
        signal: AbortSignal.timeout(1_000),
      });
      if (response.ok) return;
    } catch {
      // Vite is still starting; the supervisor also races its exit promise.
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error('Vite did not become healthy on http://127.0.0.1:5173 within 30 seconds');
}

function startVite() {
  return spawn(process.execPath, [VITE_ENTRY, '--host', '127.0.0.1', '--port', '5173'], {
    cwd: join(ROOT, 'ui'),
    stdio: 'inherit',
    env,
  });
}

function startTauri() {
  const restartSignal = createRestartSignal(restartControlDirectory);
  const child = spawn(process.execPath, [TAURI_ENTRY, ...tauriArgs], {
    cwd: ROOT,
    stdio: 'inherit',
    env: { ...env, ...restartSignal.env },
  });
  return {
    child,
    consumeRestartSignal: restartSignal.consume,
    cleanup: restartSignal.cleanup,
  };
}

const supervisor = createSupervisor({
  startVite,
  startTauri,
  waitForVite,
  stopProcess,
  log: console.log,
});

let signalHandled = false;
const handleSignal = (signal) => {
  if (signalHandled) return;
  signalHandled = true;
  console.log(`[run-dev] ${signal}; stopping Tauri and Vite`);
  void supervisor.stop();
};

process.on('SIGINT', () => handleSignal('SIGINT'));
process.on('SIGTERM', () => handleSignal('SIGTERM'));

let exitCode = 1;
try {
  exitCode = await supervisor.run();
} catch (error) {
  console.error('[run-dev] supervisor failed:', error instanceof Error ? error.message : error);
} finally {
  await supervisor.stop();
  removeRestartControlDirectory(restartControlDirectory);
  devSessionLock.release();
}
process.exitCode = exitCode;
