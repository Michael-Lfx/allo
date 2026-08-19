import { describe, expect, test } from 'bun:test';
import { isActionImitationWorkflow, normalizeWorkflow } from './workflowKind';

describe('normalizeWorkflow', () => {
  test('maps action imitation aliases', () => {
    expect(normalizeWorkflow('action2video')).toBe('action2video');
    expect(normalizeWorkflow('action2_video')).toBe('action2video');
    expect(normalizeWorkflow('imitate2video')).toBe('action2video');
    expect(normalizeWorkflow('motion_imitation')).toBe('action2video');
    expect(isActionImitationWorkflow('action')).toBe(true);
    expect(isActionImitationWorkflow('idea2video')).toBe(false);
  });
});
