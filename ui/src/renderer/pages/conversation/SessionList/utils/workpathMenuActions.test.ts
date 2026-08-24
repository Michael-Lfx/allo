import { describe, expect, test } from 'bun:test';

import { getWorkpathMenuActionKeys } from './workpathMenuActions';

describe('workpath more-menu action policy', () => {
  test('keeps the default workspace limited to pinning', () => {
    expect(
      getWorkpathMenuActionKeys({
        isDefault: true,
        isProjectWorkpath: false,
        canRemoveProjectWorkpath: false,
      })
    ).toEqual(['pin']);
  });

  test('offers copy and pin for a regular real workspace', () => {
    expect(
      getWorkpathMenuActionKeys({
        isDefault: false,
        isProjectWorkpath: false,
        canRemoveProjectWorkpath: false,
      })
    ).toEqual(['copy', 'pin']);
  });

  test('offers removal only for removable project workspaces', () => {
    expect(
      getWorkpathMenuActionKeys({
        isDefault: false,
        isProjectWorkpath: true,
        canRemoveProjectWorkpath: true,
      })
    ).toEqual(['copy', 'pin', 'remove']);

    expect(
      getWorkpathMenuActionKeys({
        isDefault: false,
        isProjectWorkpath: true,
        canRemoveProjectWorkpath: false,
      })
    ).toEqual(['copy', 'pin']);
  });
});
