/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export type StreamingMarkdownTailKind = 'text' | 'code';

export type StreamingMarkdownParts = {
  stablePrefix: string;
  tail: string;
  tailKind: StreamingMarkdownTailKind;
  codeLanguage?: string;
  codeContent?: string;
};

type FenceState = {
  marker: '`' | '~';
  length: number;
  start: number;
};

const FENCE_LINE_PATTERN = /^\s*(`{3,}|~{3,})(.*)$/;
const FENCE_CLOSE_PATTERN = /^\s*(`{3,}|~{3,})\s*$/;

const isFenceClose = (line: string, fence: FenceState): boolean => {
  const match = line.match(FENCE_CLOSE_PATTERN);
  if (!match) return false;
  return match[1][0] === fence.marker && match[1].length >= fence.length;
};

const getFenceLanguage = (openingLine: string): string => {
  const match = openingLine.match(FENCE_LINE_PATTERN);
  const language = match?.[2]?.trim().split(/\s+/)[0];
  return language || 'text';
};

/**
 * Keeps completed Markdown blocks stable while the final block is still being
 * generated. An open fenced block is exposed as a lightweight code preview so
 * it does not flash raw fence markers into the conversation bubble.
 */
export const splitStreamingMarkdown = (source: string): StreamingMarkdownParts => {
  let fence: FenceState | undefined;
  let fenceStart = -1;
  let stableBoundary = 0;
  let offset = 0;

  for (const line of source.split('\n')) {
    const lineEnd = offset + line.length;
    const nextOffset = lineEnd < source.length ? lineEnd + 1 : lineEnd;

    if (fence) {
      if (isFenceClose(line, fence)) {
        fence = undefined;
        stableBoundary = nextOffset;
      }
    } else {
      const opening = line.match(FENCE_LINE_PATTERN);
      if (opening) {
        fence = {
          marker: opening[1][0] as '`' | '~',
          length: opening[1].length,
          start: offset,
        };
        fenceStart = offset;
      } else if (line.trim() === '' && offset > 0) {
        stableBoundary = nextOffset;
      }
    }

    offset = nextOffset;
  }

  if (fence && fenceStart >= stableBoundary) {
    const stablePrefix = source.slice(0, fenceStart);
    const tail = source.slice(fenceStart);
    const firstLineEnd = tail.indexOf('\n');
    const openingLine = firstLineEnd >= 0 ? tail.slice(0, firstLineEnd) : tail;
    const codeContent = firstLineEnd >= 0 ? tail.slice(firstLineEnd + 1) : '';

    return {
      stablePrefix,
      tail,
      tailKind: 'code',
      codeLanguage: getFenceLanguage(openingLine),
      codeContent,
    };
  }

  const stablePrefix = source.slice(0, stableBoundary);
  return {
    stablePrefix,
    tail: source.slice(stableBoundary),
    tailKind: 'text',
  };
};
