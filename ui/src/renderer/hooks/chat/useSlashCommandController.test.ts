import { describe, expect, test } from 'bun:test';
import { matchSlashQuery } from './useSlashCommandController';

describe('matchSlashQuery', () => {
  test('opens for a slash token at the start of the draft', () => {
    expect(matchSlashQuery('/')).toBe('');
    expect(matchSlashQuery('/skills')).toBe('skills');
  });

  test('opens for a slash token after whitespace', () => {
    expect(matchSlashQuery('Please use /skills')).toBe('skills');
  });

  test('does not treat URLs, paths, or completed text as slash commands', () => {
    expect(matchSlashQuery('https://example.com')).toBeNull();
    expect(matchSlashQuery('C:/work/project')).toBeNull();
    expect(matchSlashQuery('Please use /skills now')).toBeNull();
  });

  test('uses the current caret instead of requiring the token at the end of the draft', () => {
    expect(matchSlashQuery('你好/skills 后续', 9)).toBe('skills');
    expect(matchSlashQuery('你好/skills 后续')).toBeNull();
  });
});
