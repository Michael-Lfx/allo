/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { parsePatch, structuredPatch } from 'diff';

export type InlineDiffLineKind = 'context' | 'insert' | 'delete';

export type InlineDiffLine = {
  kind: InlineDiffLineKind;
  text: string;
  oldLine?: number;
  newLine?: number;
};

export type InlineDiffHunk = {
  header: string;
  lines: InlineDiffLine[];
};

export const INLINE_DIFF_COLLAPSE_LINE_THRESHOLD = 24;

type StructuredHunk = {
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  lines: string[];
};

const hunkFromStructured = (hunk: StructuredHunk): InlineDiffHunk => {
  let oldLine = hunk.oldStart;
  let newLine = hunk.newStart;
  const lines: InlineDiffLine[] = hunk.lines.map((raw) => {
    const marker = raw.charAt(0);
    const text = marker === '+' || marker === '-' || marker === ' ' ? raw.slice(1) : raw;
    if (marker === '+') {
      const line: InlineDiffLine = { kind: 'insert', text, newLine };
      newLine += 1;
      return line;
    }
    if (marker === '-') {
      const line: InlineDiffLine = { kind: 'delete', text, oldLine };
      oldLine += 1;
      return line;
    }
    const line: InlineDiffLine = { kind: 'context', text, oldLine, newLine };
    oldLine += 1;
    newLine += 1;
    return line;
  });

  return {
    header: `@@ -${hunk.oldStart},${hunk.oldLines} +${hunk.newStart},${hunk.newLines} @@`,
    lines,
  };
};

export const hunksFromOldNew = (oldText: string, newText: string, context = 3): InlineDiffHunk[] => {
  const patch = structuredPatch('a', 'b', oldText, newText, undefined, undefined, { context });
  return (patch.hunks ?? []).map((hunk) => hunkFromStructured(hunk));
};

export const hunksFromUnifiedDiff = (diffText: string): InlineDiffHunk[] => {
  if (!diffText.trim()) return [];
  try {
    return parsePatch(diffText).flatMap((file) => (file.hunks ?? []).map((hunk) => hunkFromStructured(hunk)));
  } catch {
    return [];
  }
};

export const countDiffStats = (hunks: InlineDiffHunk[]): { insertions: number; deletions: number } => {
  let insertions = 0;
  let deletions = 0;
  for (const hunk of hunks) {
    for (const line of hunk.lines) {
      if (line.kind === 'insert') insertions += 1;
      if (line.kind === 'delete') deletions += 1;
    }
  }
  return { insertions, deletions };
};

export const countDiffLines = (hunks: InlineDiffHunk[]): number =>
  hunks.reduce((sum, hunk) => sum + hunk.lines.length, 0);
