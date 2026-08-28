

import { describe, expect, test } from 'bun:test';
import {
  beginTurnTiming,
  confirmFirstValue,
  getFunnelCohort,
  hasFunnelEvent,
  hasVideoSessionEvent,
  listFunnelEvents,
  markTurnAccepted,
  markTurnFirstToken,
  markTurnIdle,
  markTurnStreamFinished,
  maybeTrackRetention,
  resetFunnelForTests,
  resetTurnTimingForTests,
  trackFunnelEvent,
} from './productFunnel';
import {
  listQueuedVideoGrowthEventsForTests,
  resetVideoGrowthUploadForTests,
} from './videoGrowthUpload';
import { resetTelemetryForTests, setTelemetryOptOut } from './telemetry';

describe('product funnel', () => {
  test('records auth and accepted first-task events with a stable cohort', () => {
    resetFunnelForTests();
    const cohort = getFunnelCohort();
    expect(cohort === 'A' || cohort === 'B').toBe(true);
    trackFunnelEvent('auth_completed');
    trackFunnelEvent('home_interactive');
    trackFunnelEvent('task_accepted', { source: 'guid' });
    trackFunnelEvent('first_task_started', { source: 'guid' });
    expect(hasFunnelEvent('auth_completed')).toBe(true);
    expect(hasFunnelEvent('task_accepted')).toBe(true);
    expect(hasFunnelEvent('first_task_started')).toBe(true);
    const last = listFunnelEvents().at(-1);
    expect(last?.id).toBeTruthy();
    expect(last?.cohort).toBe(cohort);
    expect(last?.props?.runtime === 'desktop' || last?.props?.runtime === 'webui').toBe(true);
    expect(last?.props?.viewport === 'desktop' || last?.props?.viewport === 'mobile').toBe(true);
    expect(last?.props?.first_win_stage === 'active' || last?.props?.first_win_stage === 'completed').toBe(true);
    expect(last?.props?.source).toBe('guid');
  });

  test('does not treat first token as first value', () => {
    resetFunnelForTests();
    resetTurnTimingForTests();
    beginTurnTiming('req-1', { conversation_type: 'nomi', cold_start: true });
    expect(markTurnAccepted('req-1')).not.toBeNull();
    expect(markTurnFirstToken('req-1')).not.toBeNull();
    expect(hasFunnelEvent('first_value_confirmed')).toBe(false);
    expect(markTurnStreamFinished('req-1')).not.toBeNull();
    expect(markTurnIdle('req-1', 'completed')).not.toBeNull();
    expect(hasFunnelEvent('answer_completed')).toBe(true);
    expect(hasFunnelEvent('first_artifact_visible')).toBe(true);
    expect(hasFunnelEvent('first_value_confirmed')).toBe(false);
    expect(confirmFirstValue({ source: 'follow_up' })).not.toBeNull();
    expect(hasFunnelEvent('first_value_confirmed')).toBe(true);
    expect(hasFunnelEvent('value_confirmed')).toBe(true);
  });

  test('emits app_opened once per session instead of fake d1/d7 flags', () => {
    resetFunnelForTests();
    const first = maybeTrackRetention();
    const second = maybeTrackRetention();
    expect(first.map((event) => event.name)).toEqual(['app_opened']);
    expect(second).toEqual([]);
    expect(hasFunnelEvent('d1_retained')).toBe(false);
    expect(hasFunnelEvent('d7_retained')).toBe(false);
  });

  test('records video value once per session while preserving first-value semantics', () => {
    resetFunnelForTests();
    resetVideoGrowthUploadForTests();
    expect(
      confirmFirstValue({
        feature: 'video_generation',
        session_id: 'session-1',
        source: 'film_play',
      })
    ).not.toBeNull();
    expect(
      confirmFirstValue({
        feature: 'video_generation',
        session_id: 'session-2',
        source: 'film_reveal',
      })
    ).toBeNull();
    expect(hasVideoSessionEvent('value_confirmed', 'session-1')).toBe(true);
    expect(hasVideoSessionEvent('value_confirmed', 'session-2')).toBe(true);
    expect(
      listFunnelEvents().filter((event) => event.name === 'first_value_confirmed')
    ).toHaveLength(1);
    expect(
      listQueuedVideoGrowthEventsForTests().filter(
        (event) => event.name === 'value_confirmed'
      )
    ).toHaveLength(2);
  });

  test('queues only allow-listed video metadata', () => {
    resetFunnelForTests();
    resetVideoGrowthUploadForTests();
    trackFunnelEvent('render_started', {
      feature: 'video_generation',
      session_id: 'session-private',
      workflow: 'idea2video',
      prompt: 'must not upload',
    });
    const [queued] = listQueuedVideoGrowthEventsForTests();
    expect(queued?.properties.session_id).toBe('session-private');
    expect(queued?.properties.workflow).toBe('idea2video');
    expect('prompt' in (queued?.properties ?? {})).toBe(false);
  });

  test('opt-out skips first-party video growth upload', () => {
    resetFunnelForTests();
    resetVideoGrowthUploadForTests();
    resetTelemetryForTests();
    setTelemetryOptOut(true);
    trackFunnelEvent('render_started', {
      feature: 'video_generation',
      session_id: 'session-opt-out',
    });
    expect(listQueuedVideoGrowthEventsForTests()).toEqual([]);
    setTelemetryOptOut(false);
  });
});
