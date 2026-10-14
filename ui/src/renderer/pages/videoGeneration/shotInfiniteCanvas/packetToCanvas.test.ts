import { describe, expect, test } from 'bun:test';

import { packetHasCanvasRefs, packetSlotSignature } from './packetToCanvas';
import type { ShotRefSlot } from '../types';

function slot(partial: Partial<ShotRefSlot> & Pick<ShotRefSlot, 'slot'>): ShotRefSlot {
  return {
    role: 'portrait',
    path: null,
    unbound: false,
    label: '',
    user_override: false,
    ...partial,
  };
}

describe('packet canvas seed guards', () => {
  test('list rows without merged refs are not usable seeds', () => {
    expect(packetHasCanvasRefs({ image_refs: [], audio_refs: [] })).toBe(false);
    expect(packetHasCanvasRefs({ image_refs: [slot({ slot: 1 })], audio_refs: [] })).toBe(true);
    expect(packetHasCanvasRefs({ image_refs: [], audio_refs: [slot({ slot: 1, role: 'custom' })] })).toBe(true);
  });

  test('slot signature changes when GET adds live image refs', () => {
    expect(packetSlotSignature({ image_refs: [], audio_refs: [] })).toBe('|');
    expect(
      packetSlotSignature({
        image_refs: [slot({ slot: 1 }), slot({ slot: 2 })],
        audio_refs: [slot({ slot: 1, role: 'custom' })],
      })
    ).toBe('i1,i2|a1');
  });
});
