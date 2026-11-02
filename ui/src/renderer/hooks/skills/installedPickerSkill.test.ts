import { describe, expect, test } from 'bun:test';
import { isInstalledPickerSkill, type SkillCatalogEntry } from './useSkillCatalog';

const skill = (
  source: SkillCatalogEntry['source'],
  sourceKey?: string,
): Pick<SkillCatalogEntry, 'source' | 'sourceKey'> => ({ source, sourceKey });

describe('isInstalledPickerSkill', () => {
  test('keeps skills installed into the Flowy library', () => {
    expect(isInstalledPickerSkill(skill('user'))).toBe(true);
    expect(isInstalledPickerSkill(skill('extension', 'pack'))).toBe(true);
    expect(isInstalledPickerSkill(skill('mcp', 'server'))).toBe(true);
  });

  test('hides shipped builtins and skills that were only discovered on disk', () => {
    expect(isInstalledPickerSkill(skill('builtin'))).toBe(false);
    expect(isInstalledPickerSkill(skill('legacy'))).toBe(false);
    expect(isInstalledPickerSkill(skill('project', 'workspace'))).toBe(false);
    expect(isInstalledPickerSkill(skill('user', 'agents'))).toBe(false);
  });
});
