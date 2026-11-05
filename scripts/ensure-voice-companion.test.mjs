/**
 * Direction tests for `scripts/ensure-voice-companion.mjs` — written BEFORE the
 * script exists (red on purpose). They pin the contract described in
 * `docs/architecture/voice-agent/execution/dev-voice-companion-provisioning-fix-2026-11-04.zh.md` §5/R2.
 *
 * Contract under test:
 *  - resolve only the current name `voice-companion(.exe)`, never legacy `ai-mate`;
 *  - search the same directories the Rust resolver uses (target/build.noindex
 *    debug|release, `<triple>` layouts, apps/desktop/resources/bin);
 *  - a binary older than the crate sources is considered stale and rebuilt;
 *  - a build failure is a warning, not a blocker (dev must still start);
 *  - `--check` reports readiness with exit code 1 when nothing is resolvable.
 */
import { describe, expect, test } from 'bun:test';

import {
  COMPANION_NAMES,
  candidateRoots,
  ensureVoiceCompanion,
  isStale,
  resolveCompanion,
} from './ensure-voice-companion.mjs';

function fakeFs(entries) {
  const files = new Map(Object.entries(entries));
  return {
    exists: (path) => files.has(path),
    mtimeMs: (path) => files.get(path) ?? 0,
    files,
  };
}

function fakeSpawn(exitCode, output = '') {
  const calls = [];
  const spawn = (command, args, options) => {
    calls.push({ command, args, options });
    return { status: exitCode, stdout: output, stderr: exitCode === 0 ? '' : output };
  };
  return { spawn, calls };
}

describe('naming contract', () => {
  test('only the current name is accepted', () => {
    expect(COMPANION_NAMES).toEqual(['voice-companion', 'voice-companion.exe']);
    expect(COMPANION_NAMES.some((name) => name.startsWith('ai-mate'))).toBe(false);
  });
});

describe('candidate layout', () => {
  test('covers plain, triple and staged-resource layouts', () => {
    const roots = candidateRoots({ root: '/repo', platform: 'linux' });
    expect(roots).toContain('/repo/target/debug');
    expect(roots).toContain('/repo/target/release');
    expect(roots).toContain('/repo/build.noindex/debug');
    expect(roots).toContain('/repo/build.noindex/release');
    expect(roots).toContain('/repo/target/x86_64-unknown-linux-gnu/debug');
    expect(roots).toContain('/repo/build.noindex/x86_64-unknown-linux-gnu/release');
    expect(roots).toContain('/repo/apps/desktop/resources/bin');
  });
});

describe('resolveCompanion', () => {
  test('finds the daemon in a triple build directory', () => {
    const fs = fakeFs({
      '/repo/target/x86_64-unknown-linux-gnu/debug/voice-companion': 1,
    });
    expect(resolveCompanion({ root: '/repo', platform: 'linux', ...fs })).toBe(
      '/repo/target/x86_64-unknown-linux-gnu/debug/voice-companion',
    );
  });

  test('ignores the legacy ai-mate name', () => {
    const fs = fakeFs({
      '/repo/target/debug/ai-mate': 1,
      '/repo/target/debug/ai-mate.exe': 1,
    });
    expect(resolveCompanion({ root: '/repo', platform: 'win32', ...fs })).toBeNull();
  });
});

describe('isStale', () => {
  test('a binary older than the latest crate source is stale', () => {
    expect(isStale({ binaryMtimeMs: 100, sourceMtimeMs: 200 })).toBe(true);
    expect(isStale({ binaryMtimeMs: 200, sourceMtimeMs: 200 })).toBe(false);
    expect(isStale({ binaryMtimeMs: 300, sourceMtimeMs: 200 })).toBe(false);
  });
});

describe('ensureVoiceCompanion', () => {
  const builtBinary = '/repo/target/debug/voice-companion';

  test('builds the daemon when nothing is resolvable', () => {
    const fs = fakeFs({});
    const { spawn, calls } = fakeSpawn(0);
    const logged = [];
    const result = ensureVoiceCompanion({
      root: '/repo',
      platform: 'linux',
      exists: fs.exists,
      mtimeMs: fs.mtimeMs,
      spawn,
      log: (line) => logged.push(line),
      env: {},
    });
    expect(result).toMatchObject({ ok: true, built: true });
    expect(calls).toHaveLength(1);
    expect(calls[0].command).toBe('cargo');
    expect(calls[0].args).toContain('voice-companion');
    expect(calls[0].args).toContain('--bin');
    expect(calls[0].args).not.toContain('--target');
    expect(logged.join('\n')).toContain('voice-companion');
  });

  test('does not rebuild a fresh daemon', () => {
    const fs = fakeFs({
      [builtBinary]: 500,
      '/repo/apps/voice-companion/src/main.rs': 400,
    });
    const { spawn, calls } = fakeSpawn(0);
    const result = ensureVoiceCompanion({
      root: '/repo',
      platform: 'linux',
      exists: fs.exists,
      mtimeMs: fs.mtimeMs,
      spawn,
      log: () => {},
      env: {},
    });
    expect(result).toMatchObject({ ok: true, built: false });
    expect(calls).toHaveLength(0);
  });

  test('rebuilds a stale daemon', () => {
    const fs = fakeFs({
      [builtBinary]: 100,
      '/repo/apps/voice-companion/src/main.rs': 400,
    });
    const { spawn, calls } = fakeSpawn(0);
    const result = ensureVoiceCompanion({
      root: '/repo',
      platform: 'linux',
      exists: fs.exists,
      mtimeMs: fs.mtimeMs,
      spawn,
      log: () => {},
      env: {},
    });
    expect(result).toMatchObject({ ok: true, built: true });
    expect(calls).toHaveLength(1);
  });

  test('a failed build only warns and never blocks the dev loop', () => {
    const fs = fakeFs({});
    const { spawn } = fakeSpawn(1, 'compile error');
    const logged = [];
    const result = ensureVoiceCompanion({
      root: '/repo',
      platform: 'linux',
      exists: fs.exists,
      mtimeMs: fs.mtimeMs,
      spawn,
      log: (line) => logged.push(line),
      env: {},
    });
    expect(result.ok).toBe(false);
    expect(result.built).toBe(false);
    expect(logged.join('\n')).toContain('warning');
  });

  test('honors the skip switch', () => {
    const fs = fakeFs({});
    const { spawn, calls } = fakeSpawn(0);
    const result = ensureVoiceCompanion({
      root: '/repo',
      platform: 'linux',
      exists: fs.exists,
      mtimeMs: fs.mtimeMs,
      spawn,
      log: () => {},
      env: { NOMIFUN_SKIP_VOICE_COMPANION: '1' },
    });
    expect(result).toMatchObject({ ok: true, built: false, skipped: true });
    expect(calls).toHaveLength(0);
  });

  test('check mode fails when nothing is resolvable and passes after a build', () => {
    const missing = fakeFs({});
    const { spawn } = fakeSpawn(0);
    const first = ensureVoiceCompanion({
      root: '/repo',
      platform: 'linux',
      exists: missing.exists,
      mtimeMs: missing.mtimeMs,
      spawn,
      log: () => {},
      env: {},
      check: true,
    });
    expect(first.ok).toBe(false);
    expect(first.built).toBe(false);

    const present = fakeFs({ [builtBinary]: 1 });
    const second = ensureVoiceCompanion({
      root: '/repo',
      platform: 'linux',
      exists: present.exists,
      mtimeMs: present.mtimeMs,
      spawn,
      log: () => {},
      env: {},
      check: true,
    });
    expect(second.ok).toBe(true);
  });
});
