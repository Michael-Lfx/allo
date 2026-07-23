/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { toDisplayText } from '@/common/chat/displayText';

export type FirstWinChangedFile = {
  name: string;
  path: string;
  insertions: number;
  deletions: number;
};

export type FirstWinOutcomeSnapshot = {
  status: 'with_changes' | 'answer_ready';
  summary: string;
  files: FirstWinChangedFile[];
  hasAssistantAnswer: boolean;
};

type FileSummaryLike = {
  type: 'file_summary';
  diffs: Array<{
    file_name: string;
    fullPath: string;
    insertions: number;
    deletions: number;
  }>;
};

type AssistantTextLike = {
  type: 'text';
  position: 'left';
  content: {
    content?: unknown;
  };
};

function isFileSummary(item: unknown): item is FileSummaryLike {
  return Boolean(
    item &&
      typeof item === 'object' &&
      'type' in item &&
      (item as { type?: string }).type === 'file_summary' &&
      Array.isArray((item as FileSummaryLike).diffs)
  );
}

function isAssistantText(item: unknown): item is AssistantTextLike {
  return Boolean(
    item &&
      typeof item === 'object' &&
      'type' in item &&
      (item as { type?: string }).type === 'text' &&
      (item as { position?: string }).position === 'left' &&
      'content' in item &&
      item.content &&
      typeof item.content === 'object'
  );
}

function truncateSummary(text: string, max = 160): string {
  const normalized = text.replace(/\s+/g, ' ').trim();
  if (normalized.length <= max) return normalized;
  return `${normalized.slice(0, max - 1)}…`;
}

export function buildFirstWinOutcomeSnapshot(items: unknown[]): FirstWinOutcomeSnapshot | null {
  let summary = '';
  let hasAssistantAnswer = false;
  const files: FirstWinChangedFile[] = [];
  const seen = new Set<string>();

  for (let i = items.length - 1; i >= 0; i -= 1) {
    const item = items[i];
    if (!summary && isAssistantText(item)) {
      const raw =
        typeof item.content?.content === 'string'
          ? item.content.content
          : toDisplayText(item.content?.content ?? '');
      const next = truncateSummary(raw);
      if (next) {
        summary = next;
        hasAssistantAnswer = true;
      }
    }
    if (isFileSummary(item)) {
      for (const diff of item.diffs) {
        const key = diff.fullPath || diff.file_name;
        if (!key || seen.has(key)) continue;
        seen.add(key);
        files.push({
          name: diff.file_name || key,
          path: diff.fullPath || diff.file_name,
          insertions: diff.insertions ?? 0,
          deletions: diff.deletions ?? 0,
        });
      }
    }
  }

  if (!hasAssistantAnswer && files.length === 0) return null;

  return {
    status: files.length > 0 ? 'with_changes' : 'answer_ready',
    summary,
    files: files.slice(0, 5),
    hasAssistantAnswer,
  };
}

export function shouldShowFirstWinOutcomeCard(input: {
  isFirstWin: boolean;
  isProcessing: boolean;
  snapshot: FirstWinOutcomeSnapshot | null;
  dismissed: boolean;
}): boolean {
  if (!input.isFirstWin || input.isProcessing || input.dismissed) return false;
  return Boolean(input.snapshot?.hasAssistantAnswer || (input.snapshot?.files.length ?? 0) > 0);
}
