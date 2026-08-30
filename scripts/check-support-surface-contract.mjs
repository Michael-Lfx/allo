#!/usr/bin/env node
/** Static safety contract for the bounded support-surface Edge matrix. */

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const auditSource = readFileSync(join(root, 'scripts', 'check-support-surface.mjs'), 'utf8');
const probeSource = readFileSync(join(root, 'ui', 'src', 'renderer', 'pages', 'test', 'SupportSurfaceProbe.tsx'), 'utf8');

const requiredFragments = [
  'const activeChildren = new Set();',
  'const maxCasesPerRun = 128;',
  'const maxAttemptsPerCase = 3;',
  'const allowedHosts = new Set([\'localhost\', \'127.0.0.1\', \'[::1]\']);',
  'await cleanupActiveChildren();',
  'Edge profile cleanup was not confirmed',
  "url.hash = '/test/support-surface';",
  'support-surface-probe-result',
  'scrollOwnerCount',
  'iconCenterDeltaY',
  'support-chat-log-confirm',
  'DataTransfer',
];
const forbiddenFragments = ['--no-sandbox', 'deferred cleanup', 'http://example.com'];

const missing = requiredFragments.filter((fragment) => !auditSource.includes(fragment) && !probeSource.includes(fragment));
const unsafe = forbiddenFragments.filter((fragment) => auditSource.includes(fragment));
if (missing.length > 0 || unsafe.length > 0) {
  if (missing.length > 0) console.error(`[check:support-surface-contract] missing: ${missing.join(', ')}`);
  if (unsafe.length > 0) console.error(`[check:support-surface-contract] forbidden: ${unsafe.join(', ')}`);
  process.exit(1);
}
console.log('[check:support-surface-contract] passed: bounded, loopback-only, cleanup-confirmed support matrix');
