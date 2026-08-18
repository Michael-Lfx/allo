#!/usr/bin/env node
/**
 * Static safety contract for the optional Edge error-surface matrix.
 *
 * This gate intentionally does not start a browser. It protects the command
 * from unbounded launches, arbitrary remote URLs, and deferred profile cleanup;
 * the real DOM matrix remains a separate owner/CI acceptance step.
 */
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const source = readFileSync(join(root, 'scripts', 'check-error-surface.mjs'), 'utf8');

const requiredFragments = [
  'const activeChildren = new Set();',
  'const maxCasesPerRun = 256;',
  'const maxAttemptsPerCase = 2;',
  'const selectCases = (cases, limit) => {',
  "const allowedHosts = new Set(['localhost', '127.0.0.1', '[::1]']);",
  'await cleanupActiveChildren();',
  'Edge profile cleanup was not confirmed',
];

const forbiddenFragments = [
  "'--no-sandbox'",
  'deferred cleanup',
  '--max-runs 0',
];

const missing = requiredFragments.filter((fragment) => !source.includes(fragment));
const unsafe = forbiddenFragments.filter((fragment) => source.includes(fragment));

if (missing.length > 0 || unsafe.length > 0) {
  if (missing.length > 0) console.error(`[check:error-surface-contract] missing: ${missing.join(', ')}`);
  if (unsafe.length > 0) console.error(`[check:error-surface-contract] forbidden: ${unsafe.join(', ')}`);
  process.exit(1);
}

console.log('[check:error-surface-contract] passed: bounded, loopback-only, cleanup-confirmed Edge gate');
