/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { ToolPreparingEventData } from '@/common/protocolBindings/ToolPreparingEventData';

/** A tool call the model is still streaming. Uncommitted: it may never run. */
export type ToolPreparingHint = {
  callId: string;
  tool: string;
  target?: { kind: 'path' | 'text'; value: string };
};

const PATH_FIELDS = new Set(['file_path', 'filePath', 'path', 'file_name', 'fileName', 'relative_path', 'relativePath', 'dir']);

const TARGET_FIELDS = [
  ...PATH_FIELDS,
  'glob',
  'command',
  'cmd',
  'script',
  'pattern',
  'query',
  'url',
  'skill',
];

const MAX_TARGET_CHARS = 120;

function firstLine(value: string): string {
  const line = value.trim().split(/\r?\n/, 1)[0] ?? '';
  return line.length > MAX_TARGET_CHARS ? `${line.slice(0, MAX_TARGET_CHARS - 1)}…` : line;
}

export function toolPreparingHintFromEvent(data: unknown): ToolPreparingHint | undefined {
  if (!data || typeof data !== 'object') return undefined;
  const event = data as Partial<ToolPreparingEventData>;
  if (typeof event.call_id !== 'string' || typeof event.name !== 'string') return undefined;
  const tool = event.name.trim();
  if (!event.call_id || !tool) return undefined;

  const preview = event.preview && typeof event.preview === 'object' ? event.preview : undefined;
  for (const field of TARGET_FIELDS) {
    const value = preview?.[field];
    if (typeof value === 'string' && value.trim()) {
      const kind = PATH_FIELDS.has(field) ? 'path' : 'text';
      return { callId: event.call_id, tool, target: { kind, value: firstLine(value) } };
    }
  }
  return { callId: event.call_id, tool };
}
