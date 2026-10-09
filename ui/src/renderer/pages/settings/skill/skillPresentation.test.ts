import { describe, expect, test } from 'bun:test';
import { resolveCatalogSkillAvatar, resolveSkillAvatarSrc, skillIconPublicPath } from './skillPresentation';

describe('resolveSkillAvatarSrc', () => {
  test('prefixes same-origin API paths with the backend base URL', () => {
    expect(resolveSkillAvatarSrc('/api/skills/officecli/icon', 'http://127.0.0.1:13400')).toBe(
      'http://127.0.0.1:13400/api/skills/officecli/icon'
    );
  });

  test('keeps remote and data URLs intact', () => {
    expect(resolveSkillAvatarSrc('https://cdn.example/icon.png', 'http://127.0.0.1:13400')).toBe(
      'https://cdn.example/icon.png'
    );
    expect(resolveSkillAvatarSrc('data:image/png;base64,abc', '')).toBe('data:image/png;base64,abc');
  });

  test('returns undefined for empty avatars', () => {
    expect(resolveSkillAvatarSrc(undefined, '')).toBeUndefined();
    expect(resolveSkillAvatarSrc('  ', '')).toBeUndefined();
  });
});

describe('resolveCatalogSkillAvatar', () => {
  test('keeps an explicit catalog avatar', () => {
    expect(resolveCatalogSkillAvatar('pdf', '/api/skills/pdf/icon', 'builtin')).toBe('/api/skills/pdf/icon');
  });

  test('falls back to the public icon route for builtins without an avatar field', () => {
    expect(skillIconPublicPath('officecli')).toBe('/api/skills/officecli/icon');
    expect(resolveCatalogSkillAvatar('officecli', null, 'builtin')).toBe('/api/skills/officecli/icon');
  });

  test('does not invent an icon URL for non-builtin catalog rows', () => {
    expect(resolveCatalogSkillAvatar('local-pdf', undefined, 'user')).toBeUndefined();
  });
});
