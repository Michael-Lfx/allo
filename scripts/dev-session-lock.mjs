import { randomUUID } from 'node:crypto';
import { existsSync, lstatSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';

export const DEV_SESSION_LOCK_FILE = '.nomifun-dev-session.json';
export const DEV_SESSION_LOCK_VERSION = 1;
export const DEV_SESSION_HEARTBEAT_MS = 5_000;
export const DEV_SESSION_STALE_MS = 30_000;

function lockPath(buildDir) {
  return join(buildDir, DEV_SESSION_LOCK_FILE);
}

function atomicWrite(path, content) {
  const tempPath = `${path}.${process.pid}.${randomUUID()}.tmp`;
  writeFileSync(tempPath, content, { encoding: 'utf8', flag: 'wx' });
  try {
    renameSync(tempPath, path);
  } catch (error) {
    try { rmSync(tempPath, { force: true }); } catch { /* best effort */ }
    throw error;
  }
}

function processStartedAt(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return null;
  if (process.platform === 'win32') {
    const result = spawnSync(
      'powershell.exe',
      ['-NoProfile', '-NonInteractive', '-Command',
        `(Get-Process -Id ${pid} -ErrorAction SilentlyContinue).StartTime.ToUniversalTime().ToString('o')`],
      { encoding: 'utf8', windowsHide: true },
    );
    const value = result.status === 0 ? result.stdout.trim() : '';
    return value || null;
  }
  const result = spawnSync('ps', ['-o', 'lstart=', '-p', String(pid)], { encoding: 'utf8' });
  const value = result.status === 0 ? result.stdout.trim() : '';
  return value || null;
}

function currentProcessStartedAt() {
  return new Date(Date.now() - process.uptime() * 1000).toISOString();
}

export function readDevSessionLock(buildDir) {
  const path = lockPath(buildDir);
  try {
    if (!existsSync(path) || !lstatSync(path).isFile()) return null;
    const lock = JSON.parse(readFileSync(path, 'utf8'));
    if (lock?.version !== DEV_SESSION_LOCK_VERSION) return null;
    if (typeof lock.sessionId !== 'string' || typeof lock.processStartedAt !== 'string') return null;
    if (!Number.isInteger(lock.pid) || lock.pid <= 0) return null;
    if (typeof lock.heartbeatAt !== 'string') return null;
    return lock;
  } catch {
    return null;
  }
}

export function isDevSessionActive(lock, now = Date.now()) {
  if (!lock) return false;
  const heartbeat = Date.parse(lock.heartbeatAt);
  if (!Number.isFinite(heartbeat)) return false;
  const identity = processStartedAt(lock.pid);
  if (!identity) return false;
  const expected = Date.parse(lock.processStartedAt);
  const actual = Date.parse(identity);
  if (!Number.isFinite(expected) || !Number.isFinite(actual)) return false;
  // Allow a small platform clock/formatting difference, but never accept a
  // reused PID with a materially different process start time.
  if (Math.abs(expected - actual) > 2_000) return false;
  return now - heartbeat <= DEV_SESSION_STALE_MS;
}

export function quarantineStaleDevSession(buildDir) {
  const path = lockPath(buildDir);
  if (!existsSync(path)) return null;
  const quarantinePath = `${path}.stale.${Date.now()}`;
  try {
    renameSync(path, quarantinePath);
    return quarantinePath;
  } catch {
    return null;
  }
}

export function assertNoActiveDevSession(buildDir) {
  const lock = readDevSessionLock(buildDir);
  if (!lock) return { active: false, stale: false };
  if (isDevSessionActive(lock)) {
    const error = new Error(
      `active development session ${lock.sessionId} owns ${buildDir}; refusing build cache GC`,
    );
    error.code = 'ACTIVE_DEV_SESSION';
    throw error;
  }
  const quarantinePath = quarantineStaleDevSession(buildDir);
  return { active: false, stale: true, quarantinePath };
}

export function createDevSessionLock({ buildDir, workspaceRoot, targetDir }) {
  mkdirSync(buildDir, { recursive: true });
  const path = lockPath(buildDir);
  const sessionId = randomUUID();
  const processStarted = currentProcessStartedAt();
  const write = () => {
    atomicWrite(path, JSON.stringify({
      version: DEV_SESSION_LOCK_VERSION,
      sessionId,
      pid: process.pid,
      processStartedAt: processStarted,
      heartbeatAt: new Date().toISOString(),
      workspaceRoot,
      targetDir,
      buildDir,
    }));
  };

  if (readDevSessionLock(buildDir)) {
    assertNoActiveDevSession(buildDir);
  }
  write();
  const timer = setInterval(write, DEV_SESSION_HEARTBEAT_MS);
  timer.unref?.();

  return {
    sessionId,
    path,
    release() {
      clearInterval(timer);
      try {
        const current = readDevSessionLock(buildDir);
        if (current?.sessionId === sessionId) rmSync(path, { force: true });
      } catch { /* process shutdown must remain best effort */ }
    },
  };
}
