import { describe, expect, test } from 'bun:test';

import {
  findLowIntegrityPaths,
  outputCarriesLowLabel,
  warnOnLowIntegrityLabel,
} from './run-dev-integrity.mjs';

const LOW_OUTPUT =
  'D:\\repo NT AUTHORITY\\SYSTEM:(I)(OI)(CI)(F)\r\n                                 Mandatory Label\\Low Mandatory Level:(OI)(CI)(NW)\r\n\r\n已成功处理 1 个文件\r\n';
const MEDIUM_OUTPUT =
  'D:\\repo NT AUTHORITY\\SYSTEM:(I)(OI)(CI)(F)\r\n                                 Mandatory Label\\Medium Mandatory Level:(NW)\r\n';

function fakeSpawn(outputFor) {
  const calls = [];
  const spawn = (command, args) => {
    calls.push([command, ...args]);
    const output = outputFor(args[0]);
    if (output instanceof Error) throw output;
    return { stdout: output, stderr: '' };
  };
  return { spawn, calls };
}

function recordingLog() {
  const lines = [];
  const log = (line) => lines.push(line);
  return { lines, log };
}

describe('outputCarriesLowLabel', () => {
  test('detects the Low label regardless of the localized column header', () => {
    expect(outputCarriesLowLabel(LOW_OUTPUT)).toBe(true);
    expect(outputCarriesLowLabel(MEDIUM_OUTPUT)).toBe(false);
  });

  test('treats missing or non-string output as clean', () => {
    expect(outputCarriesLowLabel('')).toBe(false);
    expect(outputCarriesLowLabel(undefined)).toBe(false);
    expect(outputCarriesLowLabel(null)).toBe(false);
  });
});

describe('findLowIntegrityPaths', () => {
  test('does not probe on non-Windows hosts', () => {
    const { spawn, calls } = fakeSpawn(() => LOW_OUTPUT);
    const hits = findLowIntegrityPaths({ root: '/repo', platform: 'linux', spawn, exists: () => true });
    expect(hits).toEqual([]);
    expect(calls).toEqual([]);
  });

  test('reports the checkout root and skips missing paths', () => {
    const { spawn, calls } = fakeSpawn((path) =>
      path.endsWith('build.noindex') ? MEDIUM_OUTPUT : LOW_OUTPUT,
    );
    const hits = findLowIntegrityPaths({
      root: 'D:\\repo',
      platform: 'win32',
      spawn,
      exists: (path) => !path.endsWith('build.noindex'),
    });
    expect(hits).toEqual(['D:\\repo']);
    expect(calls).toEqual([['icacls', 'D:\\repo']]);
  });

  test('a failing icacls probe is not an error', () => {
    const { spawn } = fakeSpawn(() => new Error('icacls missing'));
    const hits = findLowIntegrityPaths({
      root: 'D:\\repo',
      platform: 'win32',
      spawn,
      exists: () => true,
    });
    expect(hits).toEqual([]);
  });
});

describe('warnOnLowIntegrityLabel', () => {
  test('prints an actionable warning when the label is present', () => {
    const { spawn } = fakeSpawn((path) => (path.endsWith('build.noindex') ? MEDIUM_OUTPUT : LOW_OUTPUT));
    const { lines, log } = recordingLog();
    const hits = warnOnLowIntegrityLabel({
      root: 'D:\\repo',
      platform: 'win32',
      spawn,
      exists: () => true,
      log,
      env: {},
    });
    expect(hits).toEqual(['D:\\repo']);
    const text = lines.join('\n');
    expect(text).toContain('WARNING');
    expect(text).toContain('icacls "D:\\repo" /setintegritylevel Medium /T /C /Q');
    expect(text).toContain('windows-low-integrity-dev-loop.md');
    expect(text).toContain('NOMIFUN_SKIP_INTEGRITY_CHECK=1');
  });

  test('stays silent on a clean checkout', () => {
    const { spawn } = fakeSpawn(() => MEDIUM_OUTPUT);
    const { lines, log } = recordingLog();
    const hits = warnOnLowIntegrityLabel({
      root: 'D:\\repo',
      platform: 'win32',
      spawn,
      exists: () => true,
      log,
      env: {},
    });
    expect(hits).toEqual([]);
    expect(lines).toEqual([]);
  });

  test('honors the skip switch before probing', () => {
    const { spawn, calls } = fakeSpawn(() => LOW_OUTPUT);
    const { lines, log } = recordingLog();
    const hits = warnOnLowIntegrityLabel({
      root: 'D:\\repo',
      platform: 'win32',
      spawn,
      exists: () => true,
      log,
      env: { NOMIFUN_SKIP_INTEGRITY_CHECK: '1' },
    });
    expect(hits).toEqual([]);
    expect(calls).toEqual([]);
    expect(lines).toEqual([]);
  });
});
