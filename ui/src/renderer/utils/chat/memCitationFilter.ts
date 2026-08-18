/**
 * Strip the `<nomi-mem-citation>` protocol block from assistant text.
 *
 * The model appends this block so the backend can bump memory-file usage.
 * It is not part of the visible answer. The renderer still filters it so
 * messages persisted before the backend strip stay clean.
 */

const OPEN = '<nomi-mem-citation>';
const CLOSE = '</nomi-mem-citation>';

export function hasMemCitations(content: string): boolean {
  if (!content || typeof content !== 'string') {
    return false;
  }
  return content.includes(OPEN) || content.includes(CLOSE);
}

export function stripMemCitations(content: string): string {
  if (!content || typeof content !== 'string' || !hasMemCitations(content)) {
    return content;
  }

  let out = '';
  let rest = content;
  while (true) {
    const start = rest.indexOf(OPEN);
    if (start < 0) {
      out += rest;
      break;
    }
    out += rest.slice(0, start);
    const after = rest.slice(start + OPEN.length);
    const end = after.indexOf(CLOSE);
    if (end < 0) {
      break;
    }
    rest = after.slice(end + CLOSE.length);
  }
  return out.replace(/\n{3,}/g, '\n\n');
}
