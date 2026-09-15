import { imageReferenceLabel } from '@oc/lib/image-reference-prompt';

const MENTION_QUERY = /@([^\s@,.;:!?，。；：！？、)\]}】）]*)$/;

export function homeImageLabel(index: number): string {
  return imageReferenceLabel(index);
}

export function homeImageMentionToken(index: number): string {
  return `@${homeImageLabel(index)}`;
}

export function detectHomeImageMentionQuery(
  text: string,
  cursor: number,
): { start: number; query: string } | null {
  const prefix = text.slice(0, cursor);
  const match = MENTION_QUERY.exec(prefix);
  if (!match || match.index === undefined) return null;
  return { start: match.index, query: match[1] };
}

export function insertHomeImageMention(
  text: string,
  cursor: number,
  index: number,
): { text: string; cursor: number } {
  const mention = detectHomeImageMentionQuery(text, cursor);
  const start = mention?.start ?? cursor;
  const insert = `${homeImageMentionToken(index)} `;
  const next = `${text.slice(0, start)}${insert}${text.slice(cursor)}`;
  return { text: next, cursor: start + insert.length };
}

export function retargetMentionsAfterRemove(
  text: string,
  removedIndex: number,
  oldCount: number,
): string {
  if (removedIndex < 0 || removedIndex >= oldCount) return text;
  const removedN = removedIndex + 1;
  return text
    .replace(/@图片(\d+)/g, (token, raw) => {
      const n = Number(raw);
      if (n === removedN) return '';
      if (n > removedN && n <= oldCount) return homeImageMentionToken(n - 2);
      return token;
    })
    .replace(/ {2,}/g, ' ');
}

export function materializeHomeImageMentions(text: string): string {
  return text.replace(/@图片(\d+)/g, '图片$1');
}

export function rewriteHomeImageMentionsToNodeTokens(text: string, nodeIds: string[]): string {
  return text.replace(/@图片(\d+)/g, (token, raw) => {
    const index = Number(raw) - 1;
    const nodeId = nodeIds[index];
    return nodeId ? `@[node:${nodeId}]` : token;
  });
}

export function appendHomeImageLegend(prompt: string, names: string[]): string {
  const materialized = materializeHomeImageMentions(prompt);
  if (names.length === 0) return materialized;
  const legend = names
    .map((name, index) => {
      const label = homeImageLabel(index);
      const trimmed = name.trim();
      return trimmed ? `${label} → ${trimmed}` : label;
    })
    .join('、');
  return `${materialized}\n\n图片对照：${legend}`;
}

export function splitHomeImageMentionParts(
  text: string,
): Array<{ type: 'text' | 'mention'; value: string }> {
  if (!text) return [];
  const parts: Array<{ type: 'text' | 'mention'; value: string }> = [];
  let lastIndex = 0;
  for (const match of text.matchAll(/@图片\d+/g)) {
    const index = match.index ?? 0;
    if (index > lastIndex) parts.push({ type: 'text', value: text.slice(lastIndex, index) });
    parts.push({ type: 'mention', value: match[0] });
    lastIndex = index + match[0].length;
  }
  if (lastIndex < text.length) parts.push({ type: 'text', value: text.slice(lastIndex) });
  return parts;
}

export function filterHomeImageMentionCandidates<T extends { index: number; label: string; name: string }>(
  items: T[],
  query: string,
): T[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return items;
  return items.filter((item) => {
    const haystack = `${item.label} ${item.name} ${homeImageLabel(item.index)} @${homeImageLabel(item.index)}`;
    return haystack.toLowerCase().includes(needle);
  });
}
