/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export type ObservationScanKind = 'messages' | 'tools';

export type MessagePreview =
  | { kind: 'text'; text: string }
  | { kind: 'tool_use'; name: string }
  | { kind: 'tool_result'; text: string; isError: boolean }
  | { kind: 'thinking'; text: string }
  | { kind: 'image'; mediaType: string }
  | { kind: 'empty' };

export type MessageScanRow = {
  index: number;
  role: string;
  kinds: string[];
  preview: MessagePreview;
  omittedReason?: string;
};

export type ToolDefScanRow = {
  index: number;
  name: string;
  description: string;
  deferred: boolean;
};

export type ObservationScanResult =
  | { kind: 'omitted'; reason: string }
  | { kind: 'messages'; rows: MessageScanRow[] }
  | { kind: 'tools'; rows: ToolDefScanRow[] }
  | { kind: 'unscannable' };

const PREVIEW_PRIORITY = ['text', 'tool_use', 'tool_result', 'thinking', 'image'] as const;

function asRecord(value: unknown): Record<string, unknown> | null {
  if (value != null && typeof value === 'object' && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return null;
}

export function omittedReasonOf(value: unknown): string | undefined {
  const record = asRecord(value);
  if (!record || Array.isArray(value)) return undefined;
  return typeof record.omitted_reason === 'string' ? record.omitted_reason : undefined;
}

function stringField(value: unknown): { text?: string; omitted?: string } {
  if (typeof value === 'string') return { text: value };
  const omitted = omittedReasonOf(value);
  if (omitted) return { omitted };
  return {};
}

function kindPriority(kind: string): number {
  const index = PREVIEW_PRIORITY.indexOf(kind as (typeof PREVIEW_PRIORITY)[number]);
  return index === -1 ? PREVIEW_PRIORITY.length : index;
}

function previewFromBlock(
  block: Record<string, unknown>
): { preview: MessagePreview; omitted?: string } | null {
  const omitted = omittedReasonOf(block);
  const type = typeof block.type === 'string' ? block.type : '';
  if (type === 'text') {
    const field = stringField(block.text);
    if (field.omitted) return { preview: { kind: 'empty' }, omitted: field.omitted };
    if (field.text?.trim()) return { preview: { kind: 'text', text: field.text.trim() } };
    return omitted ? { preview: { kind: 'empty' }, omitted } : null;
  }
  if (type === 'tool_use') {
    const name = typeof block.name === 'string' ? block.name.trim() : '';
    const inputOmitted = omittedReasonOf(block.input);
    if (name) {
      return { preview: { kind: 'tool_use', name }, omitted: omitted ?? inputOmitted };
    }
    return omitted || inputOmitted
      ? { preview: { kind: 'empty' }, omitted: omitted ?? inputOmitted }
      : null;
  }
  if (type === 'tool_result') {
    const field = stringField(block.content);
    const isError = block.is_error === true;
    if (field.omitted) {
      return { preview: { kind: 'tool_result', text: '', isError }, omitted: field.omitted };
    }
    return {
      preview: {
        kind: 'tool_result',
        text: field.text ? field.text.trim() : '',
        isError,
      },
      omitted,
    };
  }
  if (type === 'thinking') {
    const field = stringField(block.thinking);
    if (field.omitted) return { preview: { kind: 'empty' }, omitted: field.omitted };
    if (field.text?.trim()) {
      return { preview: { kind: 'thinking', text: field.text.trim() } };
    }
    return omitted ? { preview: { kind: 'empty' }, omitted } : null;
  }
  if (type === 'image') {
    const mediaType = typeof block.media_type === 'string' ? block.media_type : '';
    const dataOmitted = omittedReasonOf(block.data);
    return {
      preview: { kind: 'image', mediaType },
      omitted: omitted ?? dataOmitted,
    };
  }
  return omitted ? { preview: { kind: 'empty' }, omitted } : null;
}

function scanMessageContent(content: unknown): Pick<
  MessageScanRow,
  'kinds' | 'preview' | 'omittedReason'
> {
  const omitted = omittedReasonOf(content);
  if (omitted) {
    return { kinds: [], preview: { kind: 'empty' }, omittedReason: omitted };
  }
  if (typeof content === 'string') {
    return {
      kinds: content.trim() ? ['text'] : [],
      preview: content.trim()
        ? { kind: 'text', text: content.trim() }
        : { kind: 'empty' },
    };
  }
  if (!Array.isArray(content)) {
    return { kinds: [], preview: { kind: 'empty' } };
  }

  const kinds: string[] = [];
  let omittedReason: string | undefined;
  const candidates: MessagePreview[] = [];

  for (const item of content) {
    const record = asRecord(item);
    if (!record) continue;
    const type = typeof record.type === 'string' ? record.type : '';
    if (type && !kinds.includes(type)) kinds.push(type);
    const scanned = previewFromBlock(record);
    if (!scanned) continue;
    if (scanned.omitted && !omittedReason) omittedReason = scanned.omitted;
    if (scanned.preview.kind !== 'empty') candidates.push(scanned.preview);
  }

  candidates.sort((a, b) => kindPriority(a.kind) - kindPriority(b.kind));
  return {
    kinds,
    preview: candidates[0] ?? { kind: 'empty' },
    omittedReason,
  };
}

function scanMessageRow(item: unknown, index: number): MessageScanRow {
  const record = asRecord(item);
  if (!record) {
    return { index, role: '', kinds: [], preview: { kind: 'empty' } };
  }
  const omitted = omittedReasonOf(record);
  if (omitted) {
    return { index, role: '', kinds: [], preview: { kind: 'empty' }, omittedReason: omitted };
  }
  const role = typeof record.role === 'string' ? record.role : '';
  const scanned = scanMessageContent(record.content);
  return {
    index,
    role,
    kinds: scanned.kinds,
    preview: scanned.preview,
    omittedReason: scanned.omittedReason,
  };
}

function scanToolRow(item: unknown, index: number): ToolDefScanRow {
  const record = asRecord(item);
  const name = typeof record?.name === 'string' ? record.name : '';
  const description = typeof record?.description === 'string' ? record.description.trim() : '';
  return {
    index,
    name,
    description,
    deferred: record?.deferred === true,
  };
}

export function projectObservationScan(
  value: unknown,
  scan: ObservationScanKind
): ObservationScanResult {
  const omitted = omittedReasonOf(value);
  if (omitted) return { kind: 'omitted', reason: omitted };
  if (!Array.isArray(value)) return { kind: 'unscannable' };
  if (scan === 'messages') {
    return { kind: 'messages', rows: value.map(scanMessageRow) };
  }
  return { kind: 'tools', rows: value.map(scanToolRow) };
}
