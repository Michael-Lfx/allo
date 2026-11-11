import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';
import {
  dismissVoiceSession,
  getDismissedVoiceSessions,
  isVoiceSessionDismissed,
  resolveVoiceOwnerCompanionId,
} from '../services/voice/voiceCallPresence';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('useCompanionWindowsSync', () => {
  test('reconciles desktop companion windows with temporary voice presence without mutating persistent config', () => {
    const source = readSource(new URL('./useCompanionWindowsSync.ts', import.meta.url));

    expect(source.includes('useVoiceSnapshot')).toBe(true);
    expect(source.includes('isVoiceActive')).toBe(true);
    expect(source.includes('memoryOwnerId')).toBe(true);
    expect(source.includes('targetVoiceId')).toBe(true);
    expect(source.includes('isVoiceTarget')).toBe(true);
    expect(source.includes('enabled: Boolean(p.appearance.companion_enabled) || isVoiceTarget')).toBe(true);
    expect(source.includes('sync_companion_windows')).toBe(true);
    expect(source.includes('patchCompanion')).toBe(false);
    expect(source.includes('isVoiceSessionDismissed')).toBe(true);
    expect(source.includes('VOICE_DISMISSED_SESSIONS_EVENT')).toBe(true);
  });

  test('unified companion owner resolution strictly honors frozen owner (F5)', () => {
    // 1. Explicit owner resolves consistently
    expect(resolveVoiceOwnerCompanionId('custom-pet')).toBe('custom-pet');
    expect(resolveVoiceOwnerCompanionId('flowy-bolt')).toBe('flowy-bolt');

    // 2. Missing or empty owner returns null (never arbitrarily picks first pet or hardcoded bolt)
    expect(resolveVoiceOwnerCompanionId(null)).toBeNull();
    expect(resolveVoiceOwnerCompanionId(undefined)).toBeNull();
    expect(resolveVoiceOwnerCompanionId('')).toBeNull();
    expect(resolveVoiceOwnerCompanionId('   ')).toBeNull();

    // 3. Multi-pet window projection simulation
    const companions = [
      { companion_id: 'pet-a', appearance: { companion_enabled: false } },
      { companion_id: 'pet-b', appearance: { companion_enabled: false } },
    ];

    // Case A: Owner is pet-a -> only pet-a is voice target
    const targetA = resolveVoiceOwnerCompanionId('pet-a');
    const specsA = companions.map((p) => ({
      companion_id: p.companion_id,
      enabled: Boolean(p.appearance.companion_enabled) || (targetA != null && p.companion_id === targetA),
    }));
    expect(specsA).toEqual([
      { companion_id: 'pet-a', enabled: true },
      { companion_id: 'pet-b', enabled: false },
    ]);

    // Case B: Owner is absent -> neither pet is forced open
    const targetNone = resolveVoiceOwnerCompanionId(null);
    const specsNone = companions.map((p) => ({
      companion_id: p.companion_id,
      enabled: Boolean(p.appearance.companion_enabled) || (targetNone != null && p.companion_id === targetNone),
    }));
    expect(specsNone).toEqual([
      { companion_id: 'pet-a', enabled: false },
      { companion_id: 'pet-b', enabled: false },
    ]);
  });

  test('shared dismissal prevents re-showing pet during active call (R06)', () => {
    const testSession = 'session-dismiss-test-1';
    expect(isVoiceSessionDismissed(testSession)).toBe(false);

    dismissVoiceSession(testSession);
    expect(isVoiceSessionDismissed(testSession)).toBe(true);
    expect(getDismissedVoiceSessions().has(testSession)).toBe(true);
  });
});
