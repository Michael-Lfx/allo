import { describe, expect, test } from 'bun:test';

import { getWorkspaceTitleSubtitle } from './conversationLayoutClasses';

describe('conversation workspace title subtitle visibility', () => {
  test('keeps a trimmed real workspace path on desktop', () => {
    expect(getWorkspaceTitleSubtitle('  D:/projects/flowy  ', false)).toBe('D:/projects/flowy');
  });

  test('hides the workspace path on mobile pending transitions', () => {
    expect(getWorkspaceTitleSubtitle('D:/projects/flowy', true)).toBeUndefined();
  });

  test('hides empty workspace paths on every surface', () => {
    expect(getWorkspaceTitleSubtitle('   ', false)).toBeUndefined();
    expect(getWorkspaceTitleSubtitle(undefined, false)).toBeUndefined();
  });
});
