import { describe, expect, test } from 'bun:test';
import { kindFromConfirmationType, kindFromPermissionAction } from './approvalCardModel';

describe('approval kind mapping', () => {
  test('maps tool-group confirmation types onto the card without new message types', () => {
    expect(kindFromConfirmationType('edit')).toBe('edit');
    expect(kindFromConfirmationType('exec')).toBe('exec');
    expect(kindFromConfirmationType('info')).toBe('info');
    expect(kindFromConfirmationType('mcp')).toBe('mcp');
  });

  test('maps permission actions onto the same kinds and falls back to info', () => {
    expect(kindFromPermissionAction('edit')).toBe('edit');
    expect(kindFromPermissionAction('exec')).toBe('exec');
    expect(kindFromPermissionAction(undefined)).toBe('info');
    expect(kindFromPermissionAction('unknown')).toBe('info');
  });
});
