import { describe, expect, test } from 'bun:test';
import type { TFunction } from 'i18next';
import { buildSelectionActionsFixture } from './selectionActionsFixtures';

const t = ((key: string) => key) as TFunction;

describe('selection actions preview fixture', () => {
  test('exposes the five Beautiful UI labels', () => {
    const fixture = buildSelectionActionsFixture(t);
    expect(fixture.actions.map((action) => action.id)).toEqual([
      'explain',
      'improve',
      'shorten',
      'tone',
      'grammar',
    ]);
    expect(fixture.actions.map((action) => action.label)).toEqual([
      'beautifulUiPreview.fixtures.selectionActions.explain',
      'beautifulUiPreview.fixtures.selectionActions.improve',
      'beautifulUiPreview.fixtures.selectionActions.shorten',
      'beautifulUiPreview.fixtures.selectionActions.tone',
      'beautifulUiPreview.fixtures.selectionActions.grammar',
    ]);
  });
});
