export interface ComposerBeforeInputLike {
  inputType?: unknown;
  data?: unknown;
  isComposing?: unknown;
}

export interface DecodedComposerBeforeInput {
  inputType: string | null;
  data: string | null;
  isComposing: boolean;
}

export type ComposerBeforeInputAction =
  | { kind: 'browser' }
  | { kind: 'insertText'; text: string }
  | { kind: 'insertLineBreak' }
  | { kind: 'delete'; direction: 'backward' | 'forward' };

/** Decode browser beforeinput fields without assuming every engine supplies them. */
export const decodeComposerBeforeInput = (
  event: ComposerBeforeInputLike
): DecodedComposerBeforeInput => ({
  inputType: typeof event.inputType === 'string' ? event.inputType : null,
  data: typeof event.data === 'string' ? event.data : null,
  isComposing: event.isComposing === true,
});

export const resolveComposerBeforeInputAction = (
  event: DecodedComposerBeforeInput
): ComposerBeforeInputAction => {
  if (event.isComposing || event.inputType?.includes('Composition')) {
    return { kind: 'browser' };
  }
  switch (event.inputType) {
    case 'insertText':
      return event.data === null ? { kind: 'browser' } : { kind: 'insertText', text: event.data };
    case 'insertLineBreak':
    case 'insertParagraph':
      return { kind: 'insertLineBreak' };
    case 'deleteContentBackward':
      return { kind: 'delete', direction: 'backward' };
    case 'deleteContentForward':
      return { kind: 'delete', direction: 'forward' };
    default:
      return { kind: 'browser' };
  }
};
