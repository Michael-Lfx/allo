

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = () => readFileSync(new URL('./AgentSkillImportDrawer.tsx', import.meta.url), 'utf8');

describe('AgentSkillImportDrawer visual polish', () => {
  test('uses semantic surfaces and shared embedded content', () => {
    const drawer = source();

    expect(drawer.includes('border-border-1')).toBe(false);
    expect(drawer.includes('export const AgentSkillImportContent')).toBe(true);
    expect(drawer.includes('export const AgentSkillImportEmbedded')).toBe(true);
    expect(drawer.includes('bg-fill-2')).toBe(true);
  });
});
