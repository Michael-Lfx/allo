/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { beforeEach, describe, expect, test } from 'bun:test';

import { GUID_DRAFT_KEY, conversationDraftKey, useComposerDraftStore } from './composerDraftStore';

beforeEach(() => {
  useComposerDraftStore.setState({ drafts: {} });
});

describe('composerDraftStore', () => {
  test('setDraft stores text, attachments and workspace', () => {
    useComposerDraftStore.getState().setDraft(GUID_DRAFT_KEY, {
      text: 'hello',
      files: ['/tmp/a.png'],
      dir: '/work',
    });
    const draft = useComposerDraftStore.getState().drafts[GUID_DRAFT_KEY];
    expect(draft?.text).toBe('hello');
    expect(draft?.files).toEqual(['/tmp/a.png']);
    expect(draft?.dir).toBe('/work');
  });

  test('partial updates preserve the untouched fields', () => {
    useComposerDraftStore.getState().setDraft(GUID_DRAFT_KEY, { text: 'a', files: ['x'], dir: '/w' });
    useComposerDraftStore.getState().setDraft(GUID_DRAFT_KEY, { text: 'b' });
    const draft = useComposerDraftStore.getState().drafts[GUID_DRAFT_KEY];
    expect(draft?.text).toBe('b');
    expect(draft?.files).toEqual(['x']);
    expect(draft?.dir).toBe('/w');
  });

  test('empty snapshot deletes the draft', () => {
    useComposerDraftStore.getState().setDraft(GUID_DRAFT_KEY, { text: 'a' });
    useComposerDraftStore.getState().setDraft(GUID_DRAFT_KEY, { text: '', files: [], dir: '' });
    expect(useComposerDraftStore.getState().drafts[GUID_DRAFT_KEY]).toBeUndefined();
  });

  test('clearDraft removes only the targeted key', () => {
    const other = conversationDraftKey('conv-1');
    useComposerDraftStore.getState().setDraft(GUID_DRAFT_KEY, { text: 'guid' });
    useComposerDraftStore.getState().setDraft(other, { text: 'chat' });
    useComposerDraftStore.getState().clearDraft(GUID_DRAFT_KEY);
    expect(useComposerDraftStore.getState().drafts[GUID_DRAFT_KEY]).toBeUndefined();
    expect(useComposerDraftStore.getState().drafts[other]?.text).toBe('chat');
  });
});
