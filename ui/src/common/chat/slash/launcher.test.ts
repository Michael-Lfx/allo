import { describe, expect, test } from 'bun:test';
import {
  filterSlashLauncherItems,
  getActiveSlashTokenRange,
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

  test('finds and replaces the slash token at the current caret position', () => {
    expect(getActiveSlashTokenRange('你好/office 后续', 9)).toEqual({ start: 2, end: 9, query: 'office' });
    expect(replaceActiveSlashToken('你好/office 后续', '', 9)).toBe('你好 后续');
  });

  test('does not mistake URLs and paths for a slash command at the caret', () => {
    expect(getActiveSlashTokenRange('https://example.com', 19)).toBeNull();
    expect(getActiveSlashTokenRange('C:/work/project', 15)).toBeNull();
  });

  test('keeps explicit Skill selection order while deduplicating preset overlap', () => {
    expect(
      mergeSkillLoadIds(['project:workspace-a:goal', 'user:pdf'], ['user:pdf', 'extension:office:slides']),
    ).toEqual(['project:workspace-a:goal', 'user:pdf', 'extension:office:slides']);
  });
});
