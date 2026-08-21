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

export type MessageContextPreview = {
  text: string;
  omittedReason?: string;
};

export type MessageScanRow = {
  index: number;
  role: string;
  kinds: string[];
  preview: MessagePreview;
  context?: MessageContextPreview;
  omittedReason?: string;
};

export type ToolDefScanRow = {
  index: number;
  name: string;
  description: string;
  deferred: boolean;
  omittedReason?: string;
};

export type ObservationScanResult =
  | { kind: 'omitted'; reason: string }
  | { kind: 'messages'; rows: MessageScanRow[] }
  | { kind: 'tools'; rows: ToolDefScanRow[] }
  | { kind: 'unscannable' };

const PREVIEW_PRIORITY = ['text', 'tool_use', 'tool_result', 'thinking', 'image'] as const;
const CONTEXT_PREFIX = '[Context]';

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

export function joinOmittedMark(
  text: string,
  omittedReason: string | undefined,
  omittedLabel: string
): string {
  const mark = omittedReason ? `${omittedLabel} · ${omittedReason}` : '';
  return [text, mark].filter(Boolean).join(' · ');
}

function hasImages(value: unknown): boolean {
  return Array.isArray(value) && value.length > 0;
}

function omittedFromImages(images: unknown): string | undefined {
  if (!Array.isArray(images)) return undefined;
  for (const item of images) {
    const record = asRecord(item);
    if (!record) continue;
    const omitted = omittedReasonOf(record) ?? omittedReasonOf(record.data);
    if (omitted) return omitted;
  }
  return undefined;
}

function kindPriority(kind: string): number {
  const index = PREVIEW_PRIORITY.indexOf(kind as (typeof PREVIEW_PRIORITY)[number]);
  return index === -1 ? PREVIEW_PRIORITY.length : index;
}

function contextTextOf(value: string): string | null {
  const trimmed = value.trim();
  if (!trimmed.startsWith(CONTEXT_PREFIX)) return null;
  const remainder = trimmed.slice(CONTEXT_PREFIX.length);
  if (remainder && !/^\s/.test(remainder)) return null;
  return remainder.trim();
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
    const imageOmitted = omittedFromImages(block.images);
    if (field.omitted) {
      return {
        preview: { kind: 'tool_result', text: '', isError },
        omitted: field.omitted ?? omitted ?? imageOmitted,
      };
    }
    return {
      preview: {
        kind: 'tool_result',
        text: field.text ? field.text.trim() : '',
        isError,
      },
      omitted: omitted ?? imageOmitted,
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

function scanMessageContent(content: unknown, role: string): Pick<
  MessageScanRow,
  'kinds' | 'preview' | 'context' | 'omittedReason'
> {
  const omitted = omittedReasonOf(content);
  if (omitted) {
    return { kinds: [], preview: { kind: 'empty' }, omittedReason: omitted };
  }
  if (typeof content === 'string') {
    const context = role === 'user' ? contextTextOf(content) : null;
    if (context != null) {
      return {
        kinds: [],
        preview: { kind: 'empty' },
        context: { text: context },
      };
    }
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
  let context: MessageContextPreview | undefined;
  const candidates: MessagePreview[] = [];

  for (const [index, item] of content.entries()) {
    const record = asRecord(item);
    if (!record) continue;
    const type = typeof record.type === 'string' ? record.type : '';
    let isContext = false;
    // `[Context]` is a reserved turn-tail marker. The engine injects it only
    // as the first text block of a user message; do not hide ordinary text
    // that happens to use the marker in another role or block position.
    if (role === 'user' && index === 0 && type === 'text') {
      const field = stringField(record.text);
      // An omitted text field has no surviving `[Context]` marker. Keep the
      // omission visible instead of guessing that an arbitrary user block
      // was runtime context.
      if (field.text != null) {
        const text = contextTextOf(field.text);
        if (text != null) {
          isContext = true;
          const merged = [context?.text, text].filter(Boolean).join('\n');
          context = { text: merged };
        }
      }
    }
    if (!isContext && type && !kinds.includes(type)) kinds.push(type);
    if (!isContext && type === 'tool_result' && hasImages(record.images) && !kinds.includes('image')) {
      kinds.push('image');
    }
    if (isContext) continue;
    const scanned = previewFromBlock(record);
    if (!scanned) continue;
    if (scanned.omitted && !omittedReason) omittedReason = scanned.omitted;
    if (scanned.preview.kind !== 'empty') candidates.push(scanned.preview);
  }

  candidates.sort((a, b) => kindPriority(a.kind) - kindPriority(b.kind));
  const result = {
    kinds,
    preview: candidates[0] ?? { kind: 'empty' },
    omittedReason,
  };
  return context ? { ...result, context } : result;
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
  const scanned = scanMessageContent(record.content, role);
  return {
    index,
    role,
    kinds: scanned.kinds,
    preview: scanned.preview,
    ...(scanned.context ? { context: scanned.context } : {}),
    omittedReason: scanned.omittedReason,
  };
}

function scanToolRow(item: unknown, index: number): ToolDefScanRow {
  const record = asRecord(item);
  const name = typeof record?.name === 'string' ? record.name : '';
  const field = stringField(record?.description);
  return {
    index,
    name,
    description: field.text ? field.text.trim() : '',
    deferred: record?.deferred === true,
    omittedReason: field.omitted,
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
