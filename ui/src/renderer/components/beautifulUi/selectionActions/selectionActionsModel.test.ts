import { describe, expect, test } from 'bun:test';
import {
  BEAUTIFUL_UI_SELECTION_ACTION_IDS,
  selectionActionLabelKey,
} from './selectionActionsModel';

describe('selection action ids', () => {
  test('lists the five Beautiful UI rewrite actions without inventing emitters', () => {
    expect(BEAUTIFUL_UI_SELECTION_ACTION_IDS).toEqual([
      'explain',
      'improve',
      'shorten',
      'tone',
      'grammar',
    ]);
    expect(selectionActionLabelKey('quote')).toBe('common.reply');
  });
});
