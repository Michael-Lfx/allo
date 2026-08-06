import { symlinkSync, writeFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

import {
  createRestartControlDirectory,
  createRestartSignal,
  removeRestartControlDirectory,
  RESTART_MARKER_MAX_BYTES,
} from './run-dev-restart-signal.mjs';

describe('development restart signal', () => {
  test('consumes a matching marker exactly once', () => {
    const control = createRestartControlDirectory();
    try {
      const signal = createRestartSignal(control);
      writeFileSync(
        signal.markerPath,
        JSON.stringify({ version: 1, token: signal.token, app_pid: 1234 }),
      );

      expect(signal.consume()).toBe(true);
      expect(signal.consume()).toBe(false);
    } finally {
      removeRestartControlDirectory(control);
    }
  });

  test('rejects malformed, oversized, version-mismatched, and stale markers', () => {
    const control = createRestartControlDirectory();
    try {
      const signal = createRestartSignal(control);
      const cases = [
        '{not-json',
        'x'.repeat(RESTART_MARKER_MAX_BYTES + 1),
        JSON.stringify({ version: 2, token: signal.token, app_pid: 1234 }),
        JSON.stringify({ version: 1, token: 'b'.repeat(64), app_pid: 1234 }),
        JSON.stringify({ version: 1, token: signal.token, app_pid: 0 }),
      ];

      for (const contents of cases) {
        writeFileSync(signal.markerPath, contents);
        expect(signal.consume()).toBe(false);
      }
    } finally {
      removeRestartControlDirectory(control);
    }
  });

  test('does not follow a symbolic-link marker', () => {
    if (process.platform === 'win32') return;

    const control = createRestartControlDirectory();
    try {
      const signal = createRestartSignal(control);
      const target = `${signal.markerPath}.target`;
      writeFileSync(target, JSON.stringify({ version: 1, token: signal.token, app_pid: 1234 }));
      symlinkSync(target, signal.markerPath);

      expect(signal.consume()).toBe(false);
    } finally {
      removeRestartControlDirectory(control);
    }
  });
});
