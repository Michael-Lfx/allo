import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, describe, expect, test } from 'bun:test';
import {
  assertNoActiveDevSession,
  createDevSessionLock,
  isDevSessionActive,
  readDevSessionLock,
} from './dev-session-lock.mjs';

const tempDirs = [];

afterEach(() => {
  for (const dir of tempDirs.splice(0)) rmSync(dir, { recursive: true, force: true });
});

describe('development session lock', () => {
  test('writes an identity-bearing lock and releases only its own session', () => {
    const buildDir = mkdtempSync(join(tmpdir(), 'nomifun-session-test-'));
    tempDirs.push(buildDir);
    const lock = createDevSessionLock({
      buildDir,
      workspaceRoot: 'workspace',
      targetDir: 'target',
    });
    const saved = readDevSessionLock(buildDir);
    expect(saved?.sessionId).toBe(lock.sessionId);
    expect(saved?.pid).toBe(process.pid);
    expect(saved?.workspaceRoot).toBe('workspace');
    expect(isDevSessionActive(saved)).toBe(true);
    expect(() => assertNoActiveDevSession(buildDir)).toThrow('active development session');
    lock.release();
    expect(readDevSessionLock(buildDir)).toBeNull();
  });

  test('does not treat a stale or reused-pid lock as active', () => {
    const buildDir = mkdtempSync(join(tmpdir(), 'nomifun-session-stale-'));
    tempDirs.push(buildDir);
    const path = join(buildDir, '.nomifun-dev-session.json');
    writeFileSync(path, JSON.stringify({
      version: 1,
      sessionId: 'stale',
      pid: 999999,
      processStartedAt: new Date(0).toISOString(),
      heartbeatAt: new Date(0).toISOString(),
      workspaceRoot: 'workspace',
      targetDir: 'target',
      buildDir,
    }));
    const result = assertNoActiveDevSession(buildDir);
    expect(result.active).toBe(false);
    expect(result.stale).toBe(true);
    expect(result.quarantinePath).toBeTruthy();
    expect(readFileSync(result.quarantinePath, 'utf8')).toContain('"sessionId":"stale"');
  });
});
