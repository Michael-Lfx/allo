export interface MarkdownHeading {
  id: string;
  text: string;
  level: number;
}

const SLUG_PATTERN = /[^\p{L}\p{N}-]+/gu;

export function headingSlug(text: string): string {
  const cleaned = text
    .trim()
    .toLowerCase()
    .replace(/\s+/g, '-')
    .replace(SLUG_PATTERN, '')
    .replace(/-+/g, '-')
    .replace(/^-|-$/g, '');
  return cleaned || 'section';
}

export function nextHeadingId(text: string, seen: Map<string, number>): string {
  const base = headingSlug(text);
  const count = seen.get(base) ?? 0;
  seen.set(base, count + 1);
  return count === 0 ? `md-${base}` : `md-${base}-${count}`;
}

/** Plain heading text, aligned with what ReactMarkdown renders as children. */
export function plainHeadingText(raw: string): string {
  return raw
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/\[([^\]]+)\]\([^)]*\)/g, '$1')
    .replace(/[*_~`]/g, '')
    .replace(/\s+/g, ' ')
    .trim();
}

export function extractMarkdownHeadings(markdown: string): MarkdownHeading[] {
  const seen = new Map<string, number>();
  const headings: MarkdownHeading[] = [];
  let inFence = false;

  for (const line of markdown.split('\n')) {
    const leadingSpaces = line.match(/^ */)?.[0].length ?? 0;
    if (leadingSpaces >= 4) continue;
    const trimmed = line.trim();
    if (/^(```|~~~)/.test(trimmed)) {
      inFence = !inFence;
      continue;
    }
    if (inFence) continue;

    const match = /^(#{1,3})\s+(.+?)\s*#*\s*$/.exec(trimmed);
    if (!match) continue;

    const text = plainHeadingText(match[2]);
    if (!text) continue;

    headings.push({
      id: nextHeadingId(text, seen),
      text,
      level: match[1].length,
    });
  }

  return headings;
}

/**
 * Dots follow one heading level. Prefer h2 chapters, then h3, then h1.
 * A document title stays on the menu and is not repeated as a dot.
 */
export function sectionRailItems(headings: MarkdownHeading[]): {
  title?: string;
  items: MarkdownHeading[];
} {
  const countAt = (level: number) => headings.filter((heading) => heading.level === level).length;
  const level = ([2, 3, 1] as const).find((candidate) => countAt(candidate) >= 2);
  if (level == null) return { items: headings };

  const items = headings.filter((heading) => heading.level === level);
  const title = level > 1 ? headings.find((heading) => heading.level === 1)?.text : undefined;
  return { title, items };
}
