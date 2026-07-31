

import { describe, expect, test } from 'bun:test';
import { autoWorkStartDisabled, hasGuidInitialPayload, planGuidEntry } from './autoWorkEntry';

describe('hasGuidInitialPayload', () => {
  test('allows a source-qualified Skill-only first turn', () => {
    expect(hasGuidInitialPayload('', ['user:pdf'])).toBe(true);
    expect(hasGuidInitialPayload('   ', ['builtin:review'])).toBe(true);
  });

  test('requires text or at least one nonblank explicit Skill ID', () => {
    expect(hasGuidInitialPayload('', [])).toBe(false);
    expect(hasGuidInitialPayload('   ', ['  '])).toBe(false);
    expect(hasGuidInitialPayload('draft', [])).toBe(true);
  });
});

describe('planGuidEntry', () => {
  test('normal send (AutoWork off): sends the typed input as the first message', () => {
    const plan = planGuidEntry('do the thing', { enabled: false });
    expect(plan.autoWorkEntry).toBe(false);
    expect(plan.sendInitialMessage).toBe(true);
    expect(plan.conversationName).toBe('do the thing');
  });

  test('AutoWork entry (enabled + tag) does NOT send an initial message — avoids the running-turn race', () => {
    const plan = planGuidEntry('', { enabled: true, tag: 'release' });
    expect(plan.autoWorkEntry).toBe(true);
    expect(plan.sendInitialMessage).toBe(false);
    // No typed input → name falls back to the requirement tag.
    expect(plan.conversationName).toBe('release');
  });

  test('AutoWork entry keeps typed text as the conversation name but still does not send it', () => {
    const plan = planGuidEntry('  ship v2  ', { enabled: true, tag: 'release' });
    expect(plan.autoWorkEntry).toBe(true);
    expect(plan.sendInitialMessage).toBe(false);
    expect(plan.conversationName).toBe('ship v2');
  });

  test('enabled without a tag is not a valid AutoWork entry — falls back to normal send', () => {
    const plan = planGuidEntry('hello', { enabled: true });
    expect(plan.autoWorkEntry).toBe(false);
    expect(plan.sendInitialMessage).toBe(true);
    expect(plan.conversationName).toBe('hello');
  });
});

describe('autoWorkStartDisabled', () => {
  test('disabled while loading', () => {
    expect(autoWorkStartDisabled(true, { enabled: true, tag: 'release' })).toBe(true);
  });

  test('disabled when AutoWork is not a valid entry (off, or no tag)', () => {
    expect(autoWorkStartDisabled(false, { enabled: false })).toBe(true);
    expect(autoWorkStartDisabled(false, { enabled: true })).toBe(true);
  });

  test('enabled when AutoWork is on with a tag and not loading', () => {
    expect(autoWorkStartDisabled(false, { enabled: true, tag: 'release' })).toBe(false);
  });
});
