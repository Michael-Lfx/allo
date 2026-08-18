import { describe, expect, test } from 'bun:test';
import type { ComposerSkillChip } from './composerSkill';
import { handleComposerBeforeInputEvent } from './ComposerSkillTokenInput';
import {
  createComposerDraft,
  getComposerEditableTextLength,
  getComposerDocumentOffset,
  getComposerPlainTextOffset,
  getComposerDraftSkillChips,
  getComposerDraftText,
  insertComposerSkillAtRange,
  replaceComposerDraftRange,
} from './composerDraft';

const officeCli: ComposerSkillChip = {
  skillId: 'user:officecli',
  name: 'officecli',
  source: 'User',
};

describe('ComposerSkillTokenInput document model', () => {
  test('counts text committed into the initial zero-width guard', () => {
    expect(getComposerEditableTextLength('\u200B目标')).toBe(2);
  });

  test('keeps a selected Skill at the slash location instead of moving it to the input prefix', () => {
    const draft = insertComposerSkillAtRange(createComposerDraft('你好 /officecli 后续文本'), { start: 3, end: 13 }, officeCli);

    expect(draft).toEqual([
      { type: 'text', text: '你好 ' },
      { type: 'skill', skill: officeCli },
      { type: 'text', text: ' 后续文本' },
    ]);
    expect(getComposerDraftText(draft)).toBe('你好  后续文本');
    expect(getComposerDraftSkillChips(draft)).toEqual([officeCli]);
  });

  test('removes the adjacent atomic Skill with the same range used by cursor deletion', () => {
    const review: ComposerSkillChip = {
      skillId: 'builtin:code-review',
      name: 'code-review',
      source: 'Builtin',
    };
    const withSkill = insertComposerSkillAtRange(createComposerDraft('开头/inspect结尾'), { start: 2, end: 10 }, review);
    const withoutSkill = replaceComposerDraftRange(withSkill, { start: 2, end: 3 }, []);

    expect(getComposerDraftText(withoutSkill)).toBe('开头结尾');
    expect(getComposerDraftSkillChips(withoutSkill)).toEqual([]);
  });

  test('keeps source-qualified Skill IDs unique without disturbing the existing token', () => {
    const first = insertComposerSkillAtRange(createComposerDraft('/officecli then /officecli'), { start: 0, end: 10 }, officeCli);
    const second = insertComposerSkillAtRange(first, { start: 7, end: 17 }, officeCli);

    expect(getComposerDraftSkillChips(second)).toEqual([officeCli]);
    expect(getComposerDraftText(second)).toBe(' then ');
  });

  test('maps a plain-text caret across an adjacent Skill atom', () => {
    const draft = insertComposerSkillAtRange(createComposerDraft('甲/officecli乙'), { start: 1, end: 11 }, officeCli);

    expect(getComposerDocumentOffset(draft, 1)).toBe(1);
    expect(getComposerDocumentOffset(draft, 1, true)).toBe(2);
    expect(getComposerPlainTextOffset(draft, 2)).toBe(1);
  });
});

describe('ComposerSkillTokenInput beforeinput boundary', () => {
  test('delegates malformed inputType events to the browser without throwing', () => {
    const calls: string[] = [];

    let thrown: unknown;
    try {
      handleComposerBeforeInputEvent(
        { inputType: undefined, data: 'x' },
        {
          disabled: false,
          isComposing: false,
          preventDefault: () => calls.push('preventDefault'),
          replaceSelectionWithText: (text) => calls.push(`insert:${text}`),
          deleteSelection: (direction) => calls.push(`delete:${direction}`),
        }
      );
    } catch (error) {
      thrown = error;
    }
    expect(thrown).toBeUndefined();
    expect(calls).toEqual([]);
  });

  test('keeps composition and normal editing dispatch behavior intact', () => {
    const calls: string[] = [];
    const actions = {
      disabled: false,
      isComposing: false,
      preventDefault: () => calls.push('preventDefault'),
      replaceSelectionWithText: (text: string) => calls.push(`insert:${text}`),
      deleteSelection: (direction: 'backward' | 'forward') => calls.push(`delete:${direction}`),
    };

    handleComposerBeforeInputEvent({ inputType: 'insertText', isComposing: true, data: '中' }, actions);
    expect(calls).toEqual([]);

    handleComposerBeforeInputEvent({ inputType: 'insertText', data: '中' }, actions);
    handleComposerBeforeInputEvent({ inputType: 'deleteContentBackward', data: null }, actions);
    expect(calls).toEqual(['preventDefault', 'insert:中', 'preventDefault', 'delete:backward']);
  });

  test('delegates malformed insertText data to the browser', () => {
    const calls: string[] = [];
    const actions = {
      disabled: false,
      isComposing: false,
      preventDefault: () => calls.push('preventDefault'),
      replaceSelectionWithText: (text: string) => calls.push(`insert:${text}`),
      deleteSelection: (direction: 'backward' | 'forward') => calls.push(`delete:${direction}`),
    };

    handleComposerBeforeInputEvent({ inputType: 'insertText', data: undefined }, actions);
    handleComposerBeforeInputEvent(
      { inputType: 'insertText', data: 42 as unknown as string },
      actions
    );

    expect(calls).toEqual([]);
  });
});
