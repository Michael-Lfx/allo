import { describe, expect, test } from 'bun:test';
import { coalesceProgressEvents, eventElapsed, formatElapsedClock } from './progressEventElapsed';

describe('formatElapsedClock', () => {
  test('pads minutes and seconds', () => {
    expect(formatElapsedClock(0)).toBe('00:00');
    expect(formatElapsedClock(9)).toBe('00:09');
    expect(formatElapsedClock(75)).toBe('01:15');
  });

  test('includes hours after 3600s', () => {
    expect(formatElapsedClock(3600)).toBe('1:00:00');
    expect(formatElapsedClock(3661)).toBe('1:01:01');
  });
});

describe('eventElapsed', () => {
  const events = [
    { stage: 'extract_characters', at: '2026-08-18T01:00:00.000Z' },
    { stage: 'voice_profiles', at: '2026-08-18T01:00:08.000Z' },
    { stage: 'character_portraits_start', at: '2026-08-18T01:00:20.000Z' },
  ];

  test('completed items use the next event as the end', () => {
    expect(
      eventElapsed(events, 0, { busy: true, nowMs: Date.parse('2026-08-18T01:00:30.000Z') })
    ).toEqual({ secs: 8, live: false });
    expect(
      eventElapsed(events, 1, { busy: true, nowMs: Date.parse('2026-08-18T01:00:30.000Z') })
    ).toEqual({ secs: 12, live: false });
  });

  test('the last event ticks while the run is busy', () => {
    expect(
      eventElapsed(events, 2, { busy: true, nowMs: Date.parse('2026-08-18T01:00:45.000Z') })
    ).toEqual({ secs: 25, live: true });
  });

  test('the last event uses updated_at when the run has stopped', () => {
    expect(
      eventElapsed(events, 2, {
        busy: false,
        nowMs: Date.parse('2026-08-18T01:10:00.000Z'),
        updatedAt: '2026-08-18T01:00:27.000Z',
      })
    ).toEqual({ secs: 7, live: false });
  });

  test('missing timestamps yield no duration', () => {
    expect(eventElapsed([{ stage: 'extract_characters' }], 0, { busy: true, nowMs: 1 })).toEqual({
      secs: null,
      live: false,
    });
  });

  test('untilIndex spans a coalesced same-stage run', () => {
    const spam = [
      { stage: 'plan_scene', at: '2026-08-18T01:00:00.000Z' },
      { stage: 'plan_scene', at: '2026-08-18T01:00:00.100Z' },
      { stage: 'plan_scene', at: '2026-08-18T01:00:00.200Z' },
      { stage: 'planned', at: '2026-08-18T01:02:00.000Z' },
    ];
    expect(
      eventElapsed(spam, 0, {
        busy: false,
        nowMs: 0,
        untilIndex: 3,
      })
    ).toEqual({ secs: 120, live: false });
  });
});

describe('coalesceProgressEvents', () => {
  test('collapses consecutive identical stages', () => {
    const events = [
      { stage: 'plan_scene', at: 'a' },
      { stage: 'plan_scene', at: 'b' },
      { stage: 'plan_scene', at: 'c' },
      { stage: 'planned', at: 'd' },
      { stage: 'plan_scene', at: 'e' },
    ];
    expect(coalesceProgressEvents(events)).toEqual([
      { event: events[0], index: 0, count: 3 },
      { event: events[3], index: 3, count: 1 },
      { event: events[4], index: 4, count: 1 },
    ]);
  });
});
