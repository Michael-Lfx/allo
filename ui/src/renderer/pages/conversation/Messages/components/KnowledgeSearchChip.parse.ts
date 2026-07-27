
export type ParsedKnowledgeHit = {
  path: string;
  heading?: string;
  snippet?: string;
  kbId?: string;
};

export const NOMIFUN_KB_HITS_TRAILER = '__NOMIFUN_KB_HITS__';

/** Parse a hit count from the knowledge_search output text
 *  ("N result(s) for …" / "No matches …"). Count, 0 for explicit no-match,
 *  or null when undeterminable (e.g. still running). */
export function parseHitCount(output: string | undefined): number | null {
  if (!output) return null;
  const m = output.match(/^\s*(\d+)\s+result/);
  if (m) return Number(m[1]);
  if (/^\s*No matches/.test(output)) return 0;
  return null;
}

/** Prefer structured trailer from tool formatters; fall back to best-effort regex. */
export function parseHits(output: string | undefined): ParsedKnowledgeHit[] {
  if (!output) return [];

  const trailerIdx = output.indexOf(NOMIFUN_KB_HITS_TRAILER);
  if (trailerIdx >= 0) {
    const jsonPart = output.slice(trailerIdx + NOMIFUN_KB_HITS_TRAILER.length).trim();
    const jsonStart = jsonPart.indexOf('[');
    if (jsonStart >= 0) {
      try {
        const raw = JSON.parse(jsonPart.slice(jsonStart)) as Array<{
          kb_id?: string;
          rel_path?: string;
          heading?: string;
          snippet?: string;
        }>;
        if (Array.isArray(raw)) {
          return raw
            .filter((h) => typeof h?.rel_path === 'string' && h.rel_path.length > 0)
            .slice(0, 5)
            .map((h) => ({
              path: h.rel_path!,
              heading: typeof h.heading === 'string' ? h.heading : undefined,
              snippet: typeof h.snippet === 'string' ? h.snippet.slice(0, 160) : undefined,
              kbId: typeof h.kb_id === 'string' ? h.kb_id : undefined,
            }));
        }
      } catch {
        // fall through to regex
      }
    }
  }

  const hits: ParsedKnowledgeHit[] = [];
  const blocks = output.split(/\n(?=[-*•]|\d+\.|\[)/);
  for (const block of blocks) {
    const pathMatch =
      block.match(/(?:path|file|rel_path)[:\s]+([^\s,]+\.md)/i) ||
      block.match(/([A-Za-z0-9_./-]+\.md)/);
    if (!pathMatch) continue;
    const headingMatch = block.match(/(?:heading|title)[:\s]+(.+)/i);
    const snippetMatch = block.match(/(?:snippet|excerpt)[:\s]+([\s\S]+)/i);
    const kbMatch = block.match(/(?:kb[_ ]?id)[:\s]+([0-9a-f-]{20,})/i);
    hits.push({
      path: pathMatch[1].trim(),
      heading: headingMatch?.[1]?.trim(),
      snippet: snippetMatch?.[1]?.trim().slice(0, 160),
      kbId: kbMatch?.[1]?.trim(),
    });
    if (hits.length >= 5) break;
  }
  if (hits.length === 0) {
    for (const m of output.matchAll(/([A-Za-z0-9_./-]+\.md)/g)) {
      hits.push({ path: m[1] });
      if (hits.length >= 5) break;
    }
  }
  return hits;
}
