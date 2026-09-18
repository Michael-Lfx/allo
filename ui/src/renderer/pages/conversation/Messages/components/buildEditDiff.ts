/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { createTwoFilesPatch } from 'diff';
import {
  countDiffStats,
  hunksFromOldNew,
  type InlineDiffHunk,
} from '@renderer/components/beautifulUi/inlineDiff/inlineDiffModel';

export type EditDiffPreview = {
  filePath: string;
  displayName: string;
  unifiedDiff: string;
  hunks: InlineDiffHunk[];
  insertions: number;
  deletions: number;
};

const asRecord = (value: unknown): Record<string, unknown> | undefined => {
  if (value && typeof value === 'object' && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  if (typeof value !== 'string') return undefined;
  const trimmed = value.trim();
  if (!trimmed.startsWith('{')) return undefined;
  try {
    const parsed: unknown = JSON.parse(trimmed);
    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
      return parsed as Record<string, unknown>;
    }
  } catch {
    return undefined;
  }
  return undefined;
};

const stringField = (record: Record<string, unknown>, ...keys: string[]): string => {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === 'string' && value.length > 0) return value;
  }
  return '';
};

const displayNameOf = (filePath: string): string => filePath.split(/[/\\]/).pop() || filePath;

const previewFromReplacement = (filePath: string, oldText: string, newText: string): EditDiffPreview | undefined => {
  if (!filePath) return undefined;
  const hunks = hunksFromOldNew(oldText, newText);
  const { insertions, deletions } = countDiffStats(hunks);
  if (insertions === 0 && deletions === 0) return undefined;
  const displayName = displayNameOf(filePath);
  return {
    filePath,
    displayName,
    unifiedDiff: createTwoFilesPatch(displayName, displayName, oldText, newText, '', '', { context: 3 }),
    hunks,
    insertions,
    deletions,
  };
};

const mergePreviews = (filePath: string, parts: EditDiffPreview[]): EditDiffPreview | undefined => {
  if (!parts.length) return undefined;
  if (parts.length === 1) return parts[0];
  const hunks = parts.flatMap((part) => part.hunks);
  const { insertions, deletions } = countDiffStats(hunks);
  if (insertions === 0 && deletions === 0) return undefined;
  return {
    filePath,
    displayName: displayNameOf(filePath),
    unifiedDiff: parts.map((part) => part.unifiedDiff).join('\n'),
    hunks,
    insertions,
    deletions,
  };
};

export const buildEditDiffPreview = (source: unknown): EditDiffPreview | undefined => {
  const record = asRecord(source);
  if (!record) return undefined;

  const filePath = stringField(record, 'file_path', 'path', 'target');
  const content = typeof record.content === 'string' ? record.content : '';
  const oldString = typeof record.old_string === 'string' ? record.old_string : '';
  const newString = typeof record.new_string === 'string' ? record.new_string : '';
  const edits = Array.isArray(record.edits) ? record.edits : [];

  if (edits.length > 0) {
    const parts: EditDiffPreview[] = [];
    for (const item of edits) {
      const edit = asRecord(item);
      if (!edit) continue;
      const oldText = typeof edit.old_string === 'string' ? edit.old_string : '';
      const newText =
        typeof edit.new_text === 'string'
          ? edit.new_text
          : typeof edit.new_string === 'string'
            ? edit.new_string
            : '';
      const part = previewFromReplacement(filePath, oldText, newText);
      if (part) parts.push(part);
    }
    return mergePreviews(filePath, parts);
  }

  if (oldString.length > 0 || newString.length > 0) {
    return previewFromReplacement(filePath, oldString, newString);
  }

  if (content.length > 0) {
    return previewFromReplacement(filePath, '', content);
  }

  return undefined;
};
