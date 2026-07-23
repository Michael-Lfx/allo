

import { describe, expect, test } from 'bun:test';
import { prefixedId } from '@/common/utils';
import { parseMessageId } from '@/common/types/ids';
import {
  initialTurnPresentationState,
  turnPresentationReducer,
} from './turnPresentationState';

describe('turn presentation send/stop/retry races', () => {
  test('failed local submit clears active turn ids without leaving stop busy', () => {
    const requestMessageId = parseMessageId(prefixedId('msg'));
    let state = turnPresentationReducer(initialTurnPresentationState, {
      type: 'localSubmit',
      localRequestId: 'local-1',
      requestMessageId,
    });
    expect(state.showStop).toBe(true);

    state = turnPresentationReducer(state, { type: 'failed', detail: 'network' });
    expect(state.phase).toBe('failed');
    expect(state.showStop).toBe(false);
    expect(state.composerInteractive).toBe(true);
    expect(state.activeTurnId).toBeUndefined();
    expect(state.localRequestId).toBeUndefined();
  });

  test('stop during streaming cancels and restores composer', () => {
    let state = turnPresentationReducer(initialTurnPresentationState, {
      type: 'localSubmit',
      localRequestId: 'local-2',
    });
    state = turnPresentationReducer(state, {
      type: 'accepted',
      requestMessageId: parseMessageId(prefixedId('msg')),
    });
    state = turnPresentationReducer(state, { type: 'streaming' });
    expect(state.showStop).toBe(true);

    state = turnPresentationReducer(state, { type: 'cancelled' });
    expect(state.phase).toBe('cancelled');
    expect(state.showStop).toBe(false);
    expect(state.composerInteractive).toBe(true);
  });

  test('stream finish then turn idle recovers stop without waiting for idle phase', () => {
    let state = turnPresentationReducer(initialTurnPresentationState, {
      type: 'localSubmit',
      localRequestId: 'local-3',
    });
    state = turnPresentationReducer(state, { type: 'streaming' });
    state = turnPresentationReducer(state, { type: 'streamFinished' });
    expect(state.phase).toBe('finalizing');
    expect(state.showStop).toBe(false);
    expect(state.composerInteractive).toBe(true);
    expect(state.showStatusRail).toBe(true);

    state = turnPresentationReducer(state, { type: 'turnCompleted' });
    expect(state.phase).toBe('completed');
    expect(state.showStatusRail).toBe(false);
  });

  test('stream activity after a terminal segment reopens the presentation', () => {
    let state = turnPresentationReducer(initialTurnPresentationState, { type: 'streaming' });
    state = turnPresentationReducer(state, { type: 'streamFinished' });
    state = turnPresentationReducer(state, { type: 'thinking' });

    expect(state.phase).toBe('thinking');
    expect(state.streamFinished).toBe(false);
    expect(state.showStop).toBe(true);
  });
});
