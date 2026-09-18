import { describe, expect, it } from 'vitest';
import {
  asCompanionActivity,
  asCompanionMood,
  composeOverlayActivity,
  hitIntentAt,
  DEFAULT_HIT_AREAS,
} from './companionPresence';

describe('companionPresence', () => {
  it('falls back unknown mood to content', () => {
    expect(asCompanionMood('proud')).toBe('content');
    expect(asCompanionMood('happy')).toBe('happy');
  });

  it('parses extended activities', () => {
    expect(asCompanionActivity('awaiting_user')).toBe('awaiting_user');
    expect(asCompanionActivity('nope')).toBe('idle');
  });

  it('hits head before body in the default table', () => {
    expect(hitIntentAt(DEFAULT_HIT_AREAS, 0.5, 0.1)).toBe('head');
    expect(hitIntentAt(DEFAULT_HIT_AREAS, 0.5, 0.6)).toBe('body');
    expect(hitIntentAt(DEFAULT_HIT_AREAS, 0.02, 0.02)).toBe(null);
  });

  it('keeps interacting above agent busy', () => {
    expect(
      composeOverlayActivity({
        interacting: true,
        learnRunning: true,
        localTurnRunning: true,
        statusActivity: 'busy',
      }),
    ).toBe('interacting');
  });

  it('maps awaiting_user from execution snapshot', () => {
    expect(
      composeOverlayActivity({
        interacting: false,
        learnRunning: false,
        localTurnRunning: false,
        snapshot: {
          learn_running: false,
          conversation_processing: true,
          pending_confirmations: false,
          execution_status: 'awaiting_approval',
          summoned: false,
        },
      }),
    ).toBe('awaiting_user');
  });

  it('maps summoned live work to busy', () => {
    expect(
      composeOverlayActivity({
        interacting: false,
        learnRunning: false,
        localTurnRunning: false,
        snapshot: {
          learn_running: false,
          conversation_processing: false,
          pending_confirmations: false,
          summoned: true,
        },
      }),
    ).toBe('busy');
  });
});
