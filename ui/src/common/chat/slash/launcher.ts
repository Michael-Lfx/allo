export type SlashLauncherItemKind = 'system' | 'skill' | 'agent';

export interface SlashLauncherItem {
  /** Stable identity. Skill entries must use their source-qualified skill ID. */
  id: string;
  kind: SlashLauncherItemKind;
  /** Displayed without the leading slash. */
  name: string;
  description: string;
  /** Present for Skills so same-name entries remain distinguishable. */
  source?: string;
  tags?: string[];
}

export interface SlashLauncherGroup {
  kind: SlashLauncherItemKind;
  items: SlashLauncherItem[];
}

const GROUP_ORDER: SlashLauncherItemKind[] = ['system', 'skill', 'agent'];
const SLASH_TOKEN_CHAR_RE = /[a-zA-Z0-9_-]/;

export interface ActiveSlashTokenRange {
  start: number;
  end: number;
  query: string;
}

function isSlashBoundary(input: string, slashIndex: number): boolean {
  if (slashIndex === 0) {
    return true;
  }

  const previous = input[slashIndex - 1];
  // A slash attached to CJK text is a common way to invoke a command, while
  // URL and filesystem delimiters must remain ordinary text.
  return !/[a-zA-Z0-9_./:-]/.test(previous);
}

export function getActiveSlashTokenRange(input: string, caret = input.length): ActiveSlashTokenRange | null {
  const clampedCaret = Math.max(0, Math.min(caret, input.length));
  let queryStart = clampedCaret;
  while (queryStart > 0 && SLASH_TOKEN_CHAR_RE.test(input[queryStart - 1])) {
    queryStart -= 1;
  }

  const slashIndex = queryStart - 1;
  if (slashIndex < 0 || input[slashIndex] !== '/' || !isSlashBoundary(input, slashIndex)) {
    return null;
  }

  let end = clampedCaret;
  while (end < input.length && SLASH_TOKEN_CHAR_RE.test(input[end])) {
    end += 1;
  }

  return {
    start: slashIndex,
    end,
    query: input.slice(slashIndex + 1, clampedCaret),
  };
}

function searchableText(item: SlashLauncherItem): string {
  return [item.name, item.description, item.source, ...(item.tags ?? [])]
    .filter((value): value is string => Boolean(value))
    .join('\n')
    .toLocaleLowerCase();
}

export function filterSlashLauncherItems(items: SlashLauncherItem[], query: string): SlashLauncherItem[] {
  const normalizedQuery = query.trim().toLocaleLowerCase();
  if (!normalizedQuery) {
    return items;
  }
  return items.filter((item) => searchableText(item).includes(normalizedQuery));
}

export function groupSlashLauncherItems(items: SlashLauncherItem[]): SlashLauncherGroup[] {
  return GROUP_ORDER.flatMap((kind) => {
    const groupedItems = items.filter((item) => item.kind === kind);
    return groupedItems.length > 0 ? [{ kind, items: groupedItems }] : [];
  });
}

export function replaceActiveSlashToken(input: string, replacement = '', caret = input.length): string {
  const range = getActiveSlashTokenRange(input, caret);
  if (!range) {
    return input;
  }

  return `${input.slice(0, range.start)}${replacement}${input.slice(range.end)}`;
}

export function mergeSkillLoadIds(presetSkillIds: string[], explicitSkillIds: string[]): string[] {
  const seen = new Set<string>();
  return [...presetSkillIds, ...explicitSkillIds].filter((skillId) => {
    if (seen.has(skillId)) {
      return false;
    }
    seen.add(skillId);
    return true;
  });
}
