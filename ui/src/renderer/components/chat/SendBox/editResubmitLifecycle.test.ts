import { describe, expect, test } from 'bun:test';

import {
  clearEditResubmitComposer,
  commitComposerDraftChange,
  commitEditResubmitTerminal,
  createComposerDraftRevisionState,
  shouldCommitEditResubmitTerminal,
} from '@/renderer/components/chat/SendBox/editResubmitLifecycle';
import { resolveEditResubmitOutcome } from '@/renderer/components/chat/SendBox/editResubmitOutcome';

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

  test('user input advances revision before a deferred retry terminal can restore stale text', async () => {
    const revision = createComposerDraftRevisionState('retry text');
    const submittedRevision = revision.current;
    let composerText = 'retry text';
    let revisionObservedBySetter = submittedRevision;
    let resolveTerminal!: (status: 'post_mutation_failure') => void;
    const terminal = new Promise<'post_mutation_failure'>((resolve) => {
      resolveTerminal = resolve;
    });

    const resolved = terminal.then((status) =>
      resolveEditResubmitOutcome({
        isCurrentOperation: true,
        revisionUnchanged: revision.current === submittedRevision,
        status,
        source: 'retry',
      })
    );

    commitComposerDraftChange(revision, 'new user draft', (nextInput) => {
      revisionObservedBySetter = revision.current;
      composerText = nextInput;
    });
    resolveTerminal('post_mutation_failure');
    const outcome = await resolved;
    if (outcome.restoreSubmittedInput) composerText = 'retry text';

    expect(revisionObservedBySetter).toBeGreaterThan(submittedRevision);
    expect(composerText).toBe('new user draft');
  });
});

describe('clearEditResubmitComposer', () => {
  test('clears the token document before transient references', () => {
    const calls: string[] = [];

    clearEditResubmitComposer({
      tokenInput: { clear: () => calls.push('token-input') },
      commitEmptyDraft: () => calls.push('controlled-draft'),
      clearDomSnippets: () => calls.push('dom-snippets'),
      clearReplyQuote: () => calls.push('reply-quote'),
    });

    expect(calls).toEqual(['token-input', 'dom-snippets', 'reply-quote']);
  });

  test('uses the controlled draft fallback when the token input is absent', () => {
    const calls: string[] = [];

    clearEditResubmitComposer({
      commitEmptyDraft: () => calls.push('controlled-draft'),
      clearDomSnippets: () => calls.push('dom-snippets'),
      clearReplyQuote: () => calls.push('reply-quote'),
    });

    expect(calls).toEqual(['controlled-draft', 'dom-snippets', 'reply-quote']);
  });
});
