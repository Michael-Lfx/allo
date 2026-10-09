import { describe, expect, test } from 'bun:test';
import { deskLookTarget } from './puffGaze';

describe('deskLookTarget', () => {
  test('looks toward the pointer without spinning a full turn', () => {
    const look = deskLookTarget(1, 0, true);
    expect(look.mix).toBe(1);
    expect(look.spin).toBe(0);
    expect(look.wander).toBe(0);
    expect(look.yaw).toBeGreaterThan(0);
  });

  test('returns wander when no pointer is present', () => {
    const look = deskLookTarget(0, 0, false);
    expect(look.mix).toBe(0);
    expect(look.wander).toBe(1);
  });
});
