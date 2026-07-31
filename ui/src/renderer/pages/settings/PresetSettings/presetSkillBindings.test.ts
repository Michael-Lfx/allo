import { describe, expect, test } from 'bun:test';
import {
  pendingSkillSelectionId,
  resolvePresetSkillIdsForSave,
} from './presetSkillBindings';
import type { PendingSkill, PresetSkillCatalogItem } from './types';

const catalog: PresetSkillCatalogItem[] = [
  {
    skill_id: 'builtin:writer',
    name: 'writer',
    description: 'Built-in writing workflow',
    source: 'builtin',
  },
  {
    skill_id: 'user:writer',
    name: 'writer',
    description: 'User writing workflow',
    source: 'user',
  },
];

describe('preset catalog Skill binding selection', () => {
  test('retains same-name builtin and user Skills as distinct bindings', () => {
    const selection = resolvePresetSkillIdsForSave(
      ['builtin:writer', 'user:writer'],
      [],
      catalog,
    );

    expect(selection).toEqual({
      skillIds: ['builtin:writer', 'user:writer'],
      unresolvedPendingSkillNames: [],
    });
  });

  test('converts a checked pending import into its user catalog ID', () => {
    const pending: PendingSkill = {
      path: 'C:/imports/writer',
      name: 'writer',
      description: 'Imported writing workflow',
    };
    const selection = resolvePresetSkillIdsForSave(
      [pendingSkillSelectionId(pending)],
      [pending],
      catalog,
    );

    expect(selection).toEqual({
      skillIds: ['user:writer'],
      unresolvedPendingSkillNames: [],
    });
  });

  test('uses the import-returned catalog ID when same-name user Skills exist', () => {
    const pending: PendingSkill = {
      path: 'C:/imports/team-pdf',
      name: 'pdf',
      description: 'Team PDF workflow',
      skillId: 'user:team%2Fpdf',
    };
    const collidingCatalog: PresetSkillCatalogItem[] = [
      {
        skill_id: 'user:personal%2Fpdf',
        name: 'pdf',
        description: 'Personal PDF workflow',
        source: 'user',
      },
      {
        skill_id: 'user:team%2Fpdf',
        name: 'pdf',
        description: 'Team PDF workflow',
        source: 'user',
      },
    ];

    const selection = resolvePresetSkillIdsForSave(
      [pendingSkillSelectionId(pending)],
      [pending],
      collidingCatalog,
    );

    expect(selection).toEqual({
      skillIds: ['user:team%2Fpdf'],
      unresolvedPendingSkillNames: [],
    });
  });

  test('does not guess between same-name user Skills without an import-returned ID', () => {
    const pending: PendingSkill = {
      path: 'C:/imports/pdf',
      name: 'pdf',
      description: 'Unknown PDF workflow',
    };
    const collidingCatalog: PresetSkillCatalogItem[] = [
      {
        skill_id: 'user:personal%2Fpdf',
        name: 'pdf',
        description: 'Personal PDF workflow',
        source: 'user',
      },
      {
        skill_id: 'user:team%2Fpdf',
        name: 'pdf',
        description: 'Team PDF workflow',
        source: 'user',
      },
    ];

    const selection = resolvePresetSkillIdsForSave(
      [pendingSkillSelectionId(pending)],
      [pending],
      collidingCatalog,
    );

    expect(selection).toEqual({
      skillIds: [],
      unresolvedPendingSkillNames: ['pdf'],
    });
  });
});
