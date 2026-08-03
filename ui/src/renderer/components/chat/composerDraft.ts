import type { ComposerSkillChip } from './composerSkill';

export const COMPOSER_SKILL_ATOM = '\uFFFC';
const COMPOSER_ZERO_WIDTH_SPACE = '\u200B';

export type ComposerDraftNode =
  | { type: 'text'; text: string }
  | { type: 'skill'; skill: ComposerSkillChip };

export type ComposerDraft = ComposerDraftNode[];

export interface ComposerDraftSelection {
  start: number;
  end: number;
}

export function getComposerEditableTextLength(text: string): number {
  return text.replaceAll(COMPOSER_ZERO_WIDTH_SPACE, '').length;
}

export function normalizeComposerDraft(nodes: ComposerDraftNode[]): ComposerDraft {
  const normalized: ComposerDraft = [];

  for (const node of nodes) {
    if (node.type === 'text') {
      if (!node.text) {
        continue;
      }
      const previous = normalized.at(-1);
      if (previous?.type === 'text') {
        previous.text += node.text;
      } else {
        normalized.push({ type: 'text', text: node.text });
      }
      continue;
    }
    normalized.push(node);
  }

  return normalized;
}

export function createComposerDraft(text = '', skills: ComposerSkillChip[] = []): ComposerDraft {
  return normalizeComposerDraft([
    ...(text ? [{ type: 'text' as const, text }] : []),
    ...skills.map((skill) => ({ type: 'skill' as const, skill })),
  ]);
}

export function getComposerDraftText(draft: ComposerDraft): string {
  return draft.flatMap((node) => (node.type === 'text' ? [node.text] : [])).join('');
}

export function getComposerDraftProjection(draft: ComposerDraft): string {
  return draft.map((node) => (node.type === 'text' ? node.text : COMPOSER_SKILL_ATOM)).join('');
}

export function getComposerDraftSkillChips(draft: ComposerDraft): ComposerSkillChip[] {
  const seen = new Set<string>();
  const skills: ComposerSkillChip[] = [];

  for (const node of draft) {
    if (node.type !== 'skill' || seen.has(node.skill.skillId)) {
      continue;
    }
    seen.add(node.skill.skillId);
    skills.push(node.skill);
  }

  return skills;
}

export function getComposerDraftLength(draft: ComposerDraft): number {
  return draft.reduce((length, node) => length + (node.type === 'text' ? node.text.length : 1), 0);
}

export function getComposerPlainTextOffset(draft: ComposerDraft, documentOffset: number): number {
  let remaining = Math.max(0, Math.min(documentOffset, getComposerDraftLength(draft)));
  let plainOffset = 0;

  for (const node of draft) {
    if (node.type === 'text') {
      const consumed = Math.min(remaining, node.text.length);
      plainOffset += consumed;
      remaining -= consumed;
      if (remaining === 0) {
        break;
      }
      continue;
    }

    if (remaining === 0) {
      break;
    }
    remaining -= 1;
  }

  return plainOffset;
}

export function getComposerDocumentOffset(draft: ComposerDraft, plainTextOffset: number, preferAfterSkills = false): number {
  const target = Math.max(0, Math.min(plainTextOffset, getComposerDraftText(draft).length));
  let plainOffset = 0;
  let documentOffset = 0;

  for (const node of draft) {
    if (node.type === 'text') {
      if (target < plainOffset + node.text.length) {
        return documentOffset + target - plainOffset;
      }
      plainOffset += node.text.length;
      documentOffset += node.text.length;
      if (target === plainOffset && !preferAfterSkills) {
        return documentOffset;
      }
      continue;
    }

    if (target === plainOffset && !preferAfterSkills) {
      return documentOffset;
    }
    documentOffset += 1;
  }

  return documentOffset;
}

function splitComposerDraftAt(draft: ComposerDraft, offset: number): [ComposerDraft, ComposerDraft] {
  const clampedOffset = Math.max(0, Math.min(offset, getComposerDraftLength(draft)));
  const before: ComposerDraftNode[] = [];
  const after: ComposerDraftNode[] = [];
  let cursor = 0;

  for (const node of draft) {
    const nodeLength = node.type === 'text' ? node.text.length : 1;
    const nodeEnd = cursor + nodeLength;

    if (nodeEnd <= clampedOffset) {
      before.push(node);
    } else if (cursor >= clampedOffset) {
      after.push(node);
    } else if (node.type === 'text') {
      const splitAt = clampedOffset - cursor;
      before.push({ type: 'text', text: node.text.slice(0, splitAt) });
      after.push({ type: 'text', text: node.text.slice(splitAt) });
    } else {
      // A Skill is a single atomic document unit. A valid caret is always
      // before or after it, so this branch is only a defensive fallback.
      after.push(node);
    }

    cursor = nodeEnd;
  }

  return [normalizeComposerDraft(before), normalizeComposerDraft(after)];
}

export function replaceComposerDraftRange(
  draft: ComposerDraft,
  selection: ComposerDraftSelection,
  replacement: ComposerDraftNode[],
): ComposerDraft {
  const start = Math.min(selection.start, selection.end);
  const end = Math.max(selection.start, selection.end);
  const [before] = splitComposerDraftAt(draft, start);
  const [, after] = splitComposerDraftAt(draft, end);
  return normalizeComposerDraft([...before, ...replacement, ...after]);
}

export function insertComposerSkillAtRange(
  draft: ComposerDraft,
  selection: ComposerDraftSelection,
  skill: ComposerSkillChip,
): ComposerDraft {
  const replacement: ComposerDraftNode[] = getComposerDraftSkillChips(draft).some(
    (candidate) => candidate.skillId === skill.skillId,
  )
    ? []
    : [{ type: 'skill', skill }];

  return replaceComposerDraftRange(draft, selection, replacement);
}
