import { describe, expect, test } from 'bun:test';

import {
  commitEditResubmitTerminal,
  shouldCommitEditResubmitTerminal,
} from '@/renderer/components/chat/SendBox/editResubmitLifecycle';

describe('commitEditResubmitTerminal', () => {
  test('accepts the current terminal exactly once without consulting controller state', () => {
    expect(shouldCommitEditResubmitTerminal('op-1', new Set(), 'op-1', false)).toBe(true);
    expect(shouldCommitEditResubmitTerminal('op-1', new Set(['op-1']), 'op-1', false)).toBe(false);
    expect(shouldCommitEditResubmitTerminal('op-2', new Set(), 'op-1', false)).toBe(false);
    expect(shouldCommitEditResubmitTerminal(null, new Set(), 'op-1', true)).toBe(true);
    expect(shouldCommitEditResubmitTerminal('op-2', new Set(), 'op-1', true)).toBe(true);
    expect(shouldCommitEditResubmitTerminal(null, new Set(['op-1']), 'op-1', true)).toBe(false);
  });

  test('rejects an A-B-A terminal replay after both operations committed', () => {
    const committed = new Set<string>();
    expect(shouldCommitEditResubmitTerminal('op-a', committed, 'op-a', true)).toBe(true);
    committed.add('op-a');
    expect(shouldCommitEditResubmitTerminal('op-b', committed, 'op-b', true)).toBe(true);
    committed.add('op-b');

    expect(shouldCommitEditResubmitTerminal('op-b', committed, 'op-a', true)).toBe(false);
  });

  test('publishes terminal before shared-state clear and controller release', () => {
    const calls: string[] = [];
    const resolution = { kind: 'success' } as const;

    expect(
      commitEditResubmitTerminal({
        event: { kind: 'terminal', operationId: 'op-1', resolution },
        publish: () => calls.push('publish'),
        onPublishError: () => calls.push('publish-error'),
        afterPublish: (published) => calls.push(`after:${published}`),
        clearSharedState: () => calls.push('clear'),
        releaseOperation: () => calls.push('release'),
      })
    ).toBe(resolution);
    expect(calls).toEqual(['publish', 'after:true', 'clear', 'release']);
  });

  test('a throwing terminal subscriber cannot block shared cleanup or release', () => {
    const calls: string[] = [];
    commitEditResubmitTerminal({
      event: {
        kind: 'terminal',
        operationId: 'op-2',
        resolution: { kind: 'post_mutation_failure', error: new Error('terminal') },
      },
      publish: () => {
        calls.push('publish');
        throw new Error('subscriber');
      },
      onPublishError: () => calls.push('publish-error'),
      afterPublish: (published) => calls.push(`after:${published}`),
      clearSharedState: () => calls.push('clear'),
      releaseOperation: () => calls.push('release'),
    });
    expect(calls).toEqual(['publish', 'publish-error', 'after:false', 'clear', 'release']);
  });
});
