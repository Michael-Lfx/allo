#!/usr/bin/env node

/**
 * Enforce the renderer → common adapter boundary for Tauri APIs.
 *
 * Renderer production code must reach the desktop shell through
 * `ui/src/common/adapter/**`. Direct `@tauri-apps/*` imports belong only to the
 * adapter (and updater already isolated there). Test files are ignored.
 */

import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const RENDERER_PREFIX = 'ui/src/renderer/';
const COMMON_PREFIX = 'ui/src/common/';
const ADAPTER_PREFIX = 'ui/src/common/adapter/';

const normalizePath = (path) => path.replaceAll('\\', '/');

const isProductionSource = (path) =>
  (path.endsWith('.ts') || path.endsWith('.tsx')) &&
  !path.endsWith('.d.ts') &&
  !path.endsWith('.test.ts') &&
  !path.endsWith('.test.tsx') &&
  !path.endsWith('.spec.ts') &&
  !path.endsWith('.spec.tsx') &&
  !path.includes('/tests/') &&
  !path.includes('/__tests__/');

const isForbiddenPath = (path) =>
  isProductionSource(path) &&
  ((path.startsWith(RENDERER_PREFIX) || path.startsWith(COMMON_PREFIX)) &&
    !path.startsWith(ADAPTER_PREFIX));

const lineNumber = (source, index) => source.slice(0, index).split('\n').length;

const snippetAt = (source, index) => {
  const start = source.lastIndexOf('\n', index - 1) + 1;
  const end = source.indexOf('\n', index);
  return source.slice(start, end === -1 ? source.length : end).trim();
};

export function scanEntries(entries) {
  const violations = [];
  for (const entry of entries) {
    const path = normalizePath(entry.path);
    if (!isForbiddenPath(path)) continue;
    const pattern = /@tauri-apps\//g;
    for (const match of entry.source.matchAll(pattern)) {
      const index = match.index ?? 0;
      violations.push({
        path,
        line: lineNumber(entry.source, index),
        snippet: snippetAt(entry.source, index),
      });
    }
  }
  return violations;
}

function workspacePaths() {
  const output = execFileSync(
    'git',
    ['ls-files', '--cached', '--others', '--exclude-standard', '-z', '--', 'ui/src'],
    { cwd: ROOT, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 }
  );
  return [...new Set(output.split('\0').filter(Boolean).map(normalizePath))];
}

function selfTest() {
  const assert = (condition, message) => {
    if (!condition) throw new Error(message);
  };

  const baseline = [
    {
      path: 'ui/src/common/adapter/tauriEvents.ts',
      source: "const { listen } = await import('@tauri-apps/api/event');",
    },
    {
      path: 'ui/src/renderer/pages/example/Example.tsx',
      source: 'const answer = 42;',
    },
    {
      path: 'ui/src/common/adapter/tauriEvents.test.ts',
      source: "await import('@tauri-apps/api/event');",
    },
  ];
  assert(scanEntries(baseline).length === 0, 'baseline should pass');

  const rendererViolation = scanEntries([
    {
      path: 'ui/src/renderer/pages/example/Example.tsx',
      source: "await import('@tauri-apps/api/window');",
    },
  ]);
  assert(rendererViolation.length === 1, 'renderer direct import should fail');
  assert(rendererViolation[0].line === 1, 'line number should be reported');

  const commonViolation = scanEntries([
    {
      path: 'ui/src/common/utils/tauri.ts',
      source: "import { invoke } from '@tauri-apps/api/core';",
    },
  ]);
  assert(commonViolation.length === 1, 'common non-adapter import should fail');
}

if (process.argv.includes('--self-test')) {
  selfTest();
  console.log('tauri renderer boundary scanner self-test passed');
  process.exit(0);
}

const entries = workspacePaths()
  .filter((path) => existsSync(resolve(ROOT, path)))
  .map((path) => ({ path, source: readFileSync(resolve(ROOT, path), 'utf8') }));

const violations = scanEntries(entries);
if (violations.length > 0) {
  for (const violation of violations) {
    console.error(
      `${violation.path}:${violation.line} renderer/common code must not import @tauri-apps directly`
    );
    if (violation.snippet) console.error(`  ${violation.snippet}`);
  }
  console.error(`tauri renderer boundary check failed: ${violations.length} violation(s)`);
  process.exit(1);
}

console.log('tauri renderer boundary check passed');