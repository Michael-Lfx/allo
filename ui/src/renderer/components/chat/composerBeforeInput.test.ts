import { describe, expect, test } from 'bun:test';

import {
  decodeComposerBeforeInput,
  resolveComposerBeforeInputAction,
} from '@/renderer/components/chat/composerBeforeInput';

describe('decodeComposerBeforeInput', () => {
  test('treats missing, undefined, and unknown input types as browser-owned input', () => {
    expect(decodeComposerBeforeInput({})).toEqual({
      inputType: null,
      data: null,
      isComposing: false,
    });
    expect(decodeComposerBeforeInput({ inputType: undefined })).toEqual({
      inputType: null,
      data: null,
      isComposing: false,
    });
    expect(decodeComposerBeforeInput({ inputType: 'insertFromYank' }).inputType).toBe('insertFromYank');
  });

  test('normalizes non-string or missing data to null', () => {
    expect(decodeComposerBeforeInput({ inputType: 'insertText' }).data).toBeNull();
    expect(decodeComposerBeforeInput({ inputType: 'insertText', data: undefined }).data).toBeNull();
    expect(decodeComposerBeforeInput({ inputType: 'insertText', data: 42 }).data).toBeNull();
    expect(decodeComposerBeforeInput({ inputType: 'insertText', data: 'a' }).data).toBe('a');
  });

  test('only accepts a literal true composition marker', () => {
    expect(
      decodeComposerBeforeInput({ inputType: 'insertCompositionText', isComposing: true }).isComposing
    ).toBe(true);
    expect(
      decodeComposerBeforeInput({ inputType: 'insertText', isComposing: 'true' }).isComposing
    ).toBe(false);
  });

  test('leaves unknown and data-less insertText events to the browser', () => {
    expect(resolveComposerBeforeInputAction(decodeComposerBeforeInput({}))).toEqual({ kind: 'browser' });
    expect(
      resolveComposerBeforeInputAction(decodeComposerBeforeInput({ inputType: 'insertFromYank' }))
    ).toEqual({ kind: 'browser' });
    expect(
      resolveComposerBeforeInputAction(decodeComposerBeforeInput({ inputType: 'insertText' }))
    ).toEqual({ kind: 'browser' });
  });

  test('preserves supported text, line break, deletion, and composition actions', () => {
    expect(
      resolveComposerBeforeInputAction(
        decodeComposerBeforeInput({ inputType: 'insertText', data: 'x' })
      )
    ).toEqual({ kind: 'insertText', text: 'x' });
    expect(
      resolveComposerBeforeInputAction(decodeComposerBeforeInput({ inputType: 'insertParagraph' }))
    ).toEqual({ kind: 'insertLineBreak' });
    expect(
      resolveComposerBeforeInputAction(
        decodeComposerBeforeInput({ inputType: 'deleteContentBackward' })
      )
    ).toEqual({ kind: 'delete', direction: 'backward' });
    expect(
      resolveComposerBeforeInputAction(
        decodeComposerBeforeInput({ inputType: 'insertCompositionText', data: '拼' })
      )
    ).toEqual({ kind: 'browser' });
  });
});
