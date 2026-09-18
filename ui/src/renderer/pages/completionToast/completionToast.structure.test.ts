import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const currentDir = dirname(fileURLToPath(import.meta.url));

const routerSource = readFileSync(
  join(currentDir, '../../components/layout/Router.tsx'),
  'utf8'
);
const pageSource = readFileSync(join(currentDir, 'index.tsx'), 'utf8');
const refreshSource = readFileSync(
  join(currentDir, '../../hooks/agent/agentDetectionRefresh.ts'),
  'utf8'
);

describe('completion toast route', () => {
  test('registers isolated fullscreen route', () => {
    expect(routerSource.includes("path='/completion-toast'")).toBe(true);
    expect(routerSource.includes('CompletionToastPage')).toBe(true);
  });

  test('opens via activate command and dismisses via dismiss command', () => {
    expect(pageSource.includes("activate_completion_toast")).toBe(true);
    expect(pageSource.includes("dismiss_completion_toast")).toBe(true);
    expect(pageSource.includes('completion-toast://show')).toBe(true);
  });

  test('skips agent auto-refresh on toast hash', () => {
    expect(refreshSource.includes("#/completion-toast")).toBe(true);
  });
});
