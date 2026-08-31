#!/usr/bin/env bun
/**
 * run-dev — long-lived cross-platform supervisor for Vite + `tauri dev`.
 *
 * Unix-style inline env (`NOMI_CHANNEL=dev tauri dev ...`) fails on Windows
 * PowerShell/CMD. This script sets the env in-process and forwards argv.
 */
import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  createRestartControlDirectory,
  createRestartSignal,
  removeRestartControlDirectory,
} from './run-dev-restart-signal.mjs';
import { createSupervisor, waitForExit } from './run-dev-supervisor.mjs';
import { createDevSessionLock } from './dev-session-lock.mjs';
import { createViteHttpReadinessProbe } from './run-dev-vite-readiness.mjs';

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

/**
 * opusic-sys builds vendored libopus via CMake. On Windows, Kitware often
 * installs cmake.exe under Program Files without adding it to PATH (same
 * issue `scripts/desktop-build-win.ps1` already handles for `build:win`).
 */
function ensureCmakeOnPath(env) {
  const pathKey = Object.keys(env).find((k) => k.toLowerCase() === 'path') ?? 'Path';
  const pathValue = env[pathKey] ?? '';
  const pathParts = pathValue.split(/[;:]/).filter(Boolean);
  const cmakeOnPath = pathParts.some((dir) => {
    try {
      return existsSync(join(dir, process.platform === 'win32' ? 'cmake.exe' : 'cmake'));
    } catch {
      return false;
    }
  });
  if (cmakeOnPath) return env;

  const candidates = [
    join(process.env.ProgramFiles || 'C:\\Program Files', 'CMake', 'bin'),
    join(process.env['ProgramFiles(x86)'] || 'C:\\Program Files (x86)', 'CMake', 'bin'),
    join(process.env.LOCALAPPDATA || '', 'Programs', 'CMake', 'bin'),
  ];
  for (const binDir of candidates) {
    if (!binDir) continue;
    const exe = join(binDir, process.platform === 'win32' ? 'cmake.exe' : 'cmake');
    if (existsSync(exe)) {
      console.log(`[run-dev] CMake: injecting ${binDir} into PATH`);
      return { ...env, [pathKey]: `${binDir};${pathValue}` };
    }
  }
  return env;
}

const env = ensureCmakeOnPath({ ...process.env, NOMI_CHANNEL: 'dev' });
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

const VITE_HOST = '127.0.0.1';
const VITE_PORT = 5173;
// Cold start on Windows commonly takes ~25s just to bind. Probe the actual
// HTTP document and the main lazy route modules before launching Tauri; a TCP
// listener alone can still serve an incomplete module graph.
const VITE_READY_TIMEOUT_MS = 120_000;
const isViteReady = createViteHttpReadinessProbe({ host: VITE_HOST, port: VITE_PORT });

function hasExited(child) {
  return child && child.exitCode !== null && child.exitCode !== undefined;
}

async function waitForVite(viteProcess) {
  const deadline = Date.now() + VITE_READY_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (hasExited(viteProcess)) {
      throw new Error(
        `Vite exited before becoming healthy (code ${viteProcess.exitCode ?? 'unknown'})`,
      );
    }
    if (await isViteReady()) {
      // A stale Vite process can answer the HTTP probe while the newly spawned
      // strictPort child is exiting. Give the child exit event a turn before
      // allowing Tauri to attach to the old module graph.
      await new Promise((resolve) => setTimeout(resolve, 50));
      if (hasExited(viteProcess)) {
        throw new Error(
          `Vite exited before becoming healthy (code ${viteProcess.exitCode ?? 'unknown'})`,
        );
      }
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(
    `Vite did not become healthy on http://${VITE_HOST}:${VITE_PORT} within ${VITE_READY_TIMEOUT_MS / 1000} seconds`,
  );
}

function startVite() {
  return spawn(process.execPath, [VITE_ENTRY, '--host', VITE_HOST, '--port', String(VITE_PORT)], {
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
