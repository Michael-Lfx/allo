import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./ModelReadinessNotice.tsx', import.meta.url), 'utf8');

describe('ModelReadinessNotice', () => {
  test('keeps model recovery inline and actionable', () => {
    expect(source.includes("role='alert'")).toBe(true);
    expect(source.includes("aria-live='polite'")).toBe(true);
    expect(source.includes("type='button'")).toBe(true);
    expect(source.includes("conversation.chat.openModelSettings")).toBe(true);
    expect(source.includes('data-testid=\'guid-model-readiness-notice\'')).toBe(true);
  });
});
