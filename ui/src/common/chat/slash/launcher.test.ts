import { describe, expect, test } from 'bun:test';
import {
  filterSlashLauncherItems,
  groupSlashLauncherItems,
  mergeSkillLoadIds,
  replaceActiveSlashToken,
  type SlashLauncherItem,
} from './launcher';

const items: SlashLauncherItem[] = [
  {
    id: 'system:goal',
    kind: 'system',
    name: 'goal',
    description: 'Set the current goal',
  },
  {
    id: 'project:workspace-a:goal',
    kind: 'skill',
    name: 'goal',
    description: 'Plan a project goal',
    source: 'Project',
    tags: ['planning'],
  },
  {
    id: 'user:pdf',
    kind: 'skill',
    name: 'pdf',
    description: 'Create and inspect PDFs',
    source: 'User',
    tags: ['documents'],
  },
];

describe('slash launcher', () => {
  test('keeps a same-name system command and Skill as separate choices', () => {
    const matches = filterSlashLauncherItems(items, 'goal');

    expect(matches.map((item) => item.id)).toEqual(['system:goal', 'project:workspace-a:goal']);
    expect(groupSlashLauncherItems(matches)).toEqual([
      { kind: 'system', items: [items[0]] },
      { kind: 'skill', items: [items[1]] },
    ]);
  });

  test('searches Skill descriptions, tags, and source labels', () => {
    expect(filterSlashLauncherItems(items, 'documents').map((item) => item.id)).toEqual(['user:pdf']);
    expect(filterSlashLauncherItems(items, 'project').map((item) => item.id)).toEqual(['project:workspace-a:goal']);
  });

  test('replaces only the active trailing slash token after selecting a Skill', () => {
    expect(replaceActiveSlashToken('/pdf')).toBe('');
    expect(replaceActiveSlashToken('Please use /pdf')).toBe('Please use ');
    expect(replaceActiveSlashToken('https://example.com')).toBe('https://example.com');
  });

  test('keeps explicit Skill selection order while deduplicating preset overlap', () => {
    expect(
      mergeSkillLoadIds(['project:workspace-a:goal', 'user:pdf'], ['user:pdf', 'extension:office:slides']),
    ).toEqual(['project:workspace-a:goal', 'user:pdf', 'extension:office:slides']);
  });
});
