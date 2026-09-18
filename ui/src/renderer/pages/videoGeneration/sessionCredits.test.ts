import { describe, expect, test } from 'bun:test';
import { creditsFromSessionEvents, resolveSessionCreditsConsumed } from './sessionCredits';

describe('resolveSessionCreditsConsumed', () => {
  test('sums distinct video_credits tasks and ignores poll snapshots', () => {
    const events = [
      {
        stage: 'video_poll',
        message: '',
        metadata: { task_id: 1, credits_consumed: 9999 },
      },
      {
        stage: 'video_credits',
        message: '',
        metadata: { task_id: 10, credits_consumed: 2000 },
      },
      {
        stage: 'video_credits',
        message: '',
        metadata: { task_id: 11, credits_consumed: 3200 },
      },
      {
        stage: 'video_credits',
        message: '',
        metadata: { task_id: 10, credits_consumed: 2000 },
      },
    ];
    expect(creditsFromSessionEvents(events)).toBe(5200);
    expect(
      resolveSessionCreditsConsumed({
        sessionCredits: 1000,
        statusCredits: 800,
        events,
      })
    ).toBe(5200);
  });

  test('falls back to the persisted session ledger', () => {
    expect(
      resolveSessionCreditsConsumed({
        sessionCredits: 4800,
        statusCredits: 0,
        events: [],
      })
    ).toBe(4800);
  });
});
