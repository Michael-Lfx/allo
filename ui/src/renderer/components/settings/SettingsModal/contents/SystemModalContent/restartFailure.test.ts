import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');
const arcoOverrides = readFileSync(new URL('../../../../../styles/arco-override.css', import.meta.url), 'utf8');

describe('work-directory restart failure contract', () => {
  test('clears the migration veil and surfaces a native restart failure', () => {
    expect(source).toContain('await ipcBridge.application.restart.invoke()');
    expect(source).toContain('catch (restartError: unknown)');
    expect(source).toContain('setIsRelocating(false);');
    expect(source).toContain('setError(restartError instanceof Error ? restartError.message : String(restartError));');
  });

  test('uses a scoped theme for the relocation confirmation without a persistent success notice', () => {
    expect(source).toContain("className: 'work-dir-relocation-confirm'");
    expect(source).not.toContain('workDirRelocationCompleted');
    expect(arcoOverrides).toContain('.arco-modal.work-dir-relocation-confirm');
    expect(arcoOverrides).toContain('var(--flowy-surface-1, var(--dialog-fill-0))');
    expect(arcoOverrides).toContain('var(--flowy-text-2, var(--text-secondary))');
  });
});
