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
const ACTIVE_SLASH_TOKEN_RE = /(?:^|\s)\/[a-zA-Z0-9_-]*$/;

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

export function replaceActiveSlashToken(input: string, replacement = ''): string {
  const match = input.match(ACTIVE_SLASH_TOKEN_RE);
  if (!match || match.index === undefined) {
    return input;
  }

  const tokenStart = match.index + (match[0].startsWith(' ') ? 1 : 0);
  return `${input.slice(0, tokenStart)}${replacement}${input.slice(tokenStart + match[0].trimStart().length)}`;
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
