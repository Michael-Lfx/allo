

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('SkillsHubSettings agent skill migration entry', () => {
  test('exposes an Agent Skills import action backed by external source detection', () => {
    const source = readSource(new URL('./SkillsHubSettings.tsx', import.meta.url));

    expect(source.includes('AgentSkillImportDrawer')).toBe(true);
    expect(source.includes("data-testid='btn-import-agent-skills'")).toBe(true);
    expect(source.includes('detectAndCountExternalSkills')).toBe(true);
    expect(source.includes('setAgentImportVisible(true)')).toBe(true);
  });

  test('groups the three import paths under one stable, non-wrapping action menu', () => {
    const source = readSource(new URL('./SkillsHubSettings.tsx', import.meta.url));

    expect(source.includes("gap-6px';")).toBe(true);
    expect(source.includes('<Dropdown')).toBe(true);
    expect(source.includes('<Menu.Item key=\'agent\'')).toBe(true);
    expect(source.includes('<Menu.Item key=\'folder\'')).toBe(true);
    expect(source.includes('<Menu.Item key=\'zip\'')).toBe(true);
    expect(source.includes('!whitespace-nowrap')).toBe(true);
  });
});
