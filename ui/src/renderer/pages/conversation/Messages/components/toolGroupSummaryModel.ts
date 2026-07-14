/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { NormalizedToolCall } from '@/common/chat/normalizeToolCall';
import type { TurnDisclosureProcessState } from '../turnDisclosureModel';
import { mergeProcessStates } from '../turnProcessState';

export interface ToolSummaryDescriptor {
  target: string;
  count: number;
}

export type ToolReceiptAction =
  | 'read_files'
  | 'edit_files'
  | 'run_commands'
  | 'search_code'
  | 'web_search'
  | 'web_extract'
  | 'list_files'
  | 'load_tools'
  | 'generic';

export type ToolReceiptIcon = 'tool' | 'file' | 'edit';

export interface ToolReceiptSummaryPart {
  action: ToolReceiptAction;
  count: number;
  state: TurnDisclosureProcessState;
  target?: string;
  skipped?: boolean;
}

export interface ToolReceiptDetailRow {
  key: string;
  action: ToolReceiptAction;
  state: TurnDisclosureProcessState;
  title: string;
  target?: string;
  input?: string;
  output?: string;
  truncated?: boolean;
  skipped?: boolean;
}

const toolReceiptIconByAction: Record<ToolReceiptAction, ToolReceiptIcon> = {
  read_files: 'file',
  edit_files: 'edit',
  run_commands: 'tool',
  search_code: 'file',
  web_search: 'tool',
  web_extract: 'tool',
  list_files: 'file',
  load_tools: 'tool',
  generic: 'tool',
};

const stateMatchesTool = (state: TurnDisclosureProcessState, tool: NormalizedToolCall): boolean => {
  if (state === 'running') return tool.status === 'running' || tool.status === 'pending';
  if (state === 'failed') return tool.status === 'error' && !tool.nonFatalFailure;
  if (state === 'canceled') return tool.status === 'canceled';
  if (state === 'completed') return tool.status === 'completed' || tool.nonFatalFailure === true;
  return tool.status === 'pending' || tool.status === 'running';
};

const compactToolText = (value?: unknown): string => {
  if (value == null) return '';
  const text =
    typeof value === 'string'
      ? value
      : (() => {
          try {
            return JSON.stringify(value, null, 2);
          } catch {
            return String(value);
          }
        })();
  return text.replace(/\s+/g, ' ').trim();
};

const formatToolTarget = (tool: NormalizedToolCall): string => {
  if (classifyToolForReceipt(tool) === 'run_commands') return getCommandTarget(tool);

  const name = compactToolText(tool.name);
  const description = compactToolText(tool.description);
  if (name && description && description !== name) return `${name} ${description}`;
  return name || description || tool.key;
};

const commandFieldNames = ['command', 'cmd', 'script', 'shell', 'bash'];
const fileFieldNames = ['file_path', 'filePath', 'path', 'file_name', 'fileName', 'relative_path', 'relativePath'];

const pickCommandFromValue = (value: unknown): string | undefined => {
  if (!value || typeof value !== 'object') return undefined;
  const record = value as Record<string, unknown>;

  for (const field of commandFieldNames) {
    const fieldValue = record[field];
    if (typeof fieldValue === 'string' && compactToolText(fieldValue)) return compactToolText(fieldValue);
  }

  for (const fieldValue of Object.values(record)) {
    if (fieldValue && typeof fieldValue === 'object') {
      const nested = pickCommandFromValue(fieldValue);
      if (nested) return nested;
    }
  }

  return undefined;
};

const extractCommandFromText = (value?: string): string | undefined => {
  const compacted = compactToolText(value);
  if (!compacted) return undefined;

  try {
    const parsed = JSON.parse(value ?? '');
    const command = pickCommandFromValue(parsed);
    if (command) return command;
    if (typeof parsed === 'string') return compactToolText(parsed);
    return undefined;
  } catch {
    // Plain shell strings are already the desired preview.
  }

  return compacted;
};

const pickFileTargetFromValue = (value: unknown): string | undefined => {
  if (!value || typeof value !== 'object') return undefined;
  const record = value as Record<string, unknown>;

  for (const field of fileFieldNames) {
    const fieldValue = record[field];
    if (typeof fieldValue === 'string' && compactToolText(fieldValue)) return compactToolText(fieldValue);
  }

  for (const fieldValue of Object.values(record)) {
    if (fieldValue && typeof fieldValue === 'object') {
      const nested = pickFileTargetFromValue(fieldValue);
      if (nested) return nested;
    }
  }

  return undefined;
};

const extractFileTargetFromText = (value?: string): string | undefined => {
  const compacted = compactToolText(value);
  if (!compacted) return undefined;

  try {
    const parsed = JSON.parse(value ?? '');
    const target = pickFileTargetFromValue(parsed);
    if (target) return target;
    if (typeof parsed === 'string') return compactToolText(parsed);
    return undefined;
  } catch {
    // Plain read/edit descriptions are already useful file previews.
  }

  return compacted;
};

const getCommandTarget = (tool: NormalizedToolCall): string => {
  const description = compactToolText(tool.description);
  const name = compactToolText(tool.name);
  if (description && description !== name) return description;
  return extractCommandFromText(tool.input) || description || name || tool.key;
};

const getFileTarget = (tool: NormalizedToolCall): string | undefined => {
  const description = compactToolText(tool.description);
  const name = compactToolText(tool.name);
  if (description && description !== name) return description;
  return extractFileTargetFromText(tool.input);
};

const normalizeToolSearchText = (value: string): string => value.replace(/[_-]+/g, ' ').toLowerCase();

const getToolSearchText = (tool: NormalizedToolCall): string =>
  normalizeToolSearchText(`${compactToolText(tool.name)} ${compactToolText(tool.description)} ${tool.key ?? ''}`);

const getToolNameSearchText = (tool: NormalizedToolCall): string =>
  normalizeToolSearchText(`${compactToolText(tool.name)} ${tool.key ?? ''}`);

const parseToolInputRecord = (tool: NormalizedToolCall): Record<string, unknown> | undefined => {
  const sources = [tool.input, tool.description].filter((value): value is string => Boolean(value));
  for (const source of sources) {
    try {
      const parsed = JSON.parse(source);
      if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
        return parsed as Record<string, unknown>;
      }
    } catch {
      // Plain-text tool descriptions are not JSON input payloads.
    }
  }
  return undefined;
};

const getWebSearchTarget = (tool: NormalizedToolCall): string | undefined => {
  const record = parseToolInputRecord(tool);
  const query = record?.query;
  return typeof query === 'string' && compactToolText(query) ? compactToolText(query) : undefined;
};

const getWebExtractTarget = (tool: NormalizedToolCall): string | undefined => {
  const record = parseToolInputRecord(tool);
  const urls = record?.urls;
  if (!Array.isArray(urls)) return undefined;
  const normalized = urls
    .filter((url): url is string => typeof url === 'string' && compactToolText(url).length > 0)
    .map((url) => compactToolText(url));
  return normalized.length ? normalized.join(', ') : undefined;
};

const classifyToolForReceipt = (tool: NormalizedToolCall): ToolReceiptAction => {
  const text = getToolSearchText(tool);
  const nameText = getToolNameSearchText(tool);
  const normalizedName = normalizeToolSearchText(compactToolText(tool.name));

  if (normalizedName === 'web search') return 'web_search';
  if (normalizedName === 'web extract') return 'web_extract';
  if (normalizedName === 'update plan') return 'generic';
  if (/\b(bash|shell|exec|execute|terminal|command|run)\b/.test(nameText)) return 'run_commands';
  if (/\b(grep|rg|search|find)\b/.test(text)) return 'search_code';
  if (/\b(glob|list|ls|directory|dir)\b/.test(text)) return 'list_files';
  if (/\b(write|edit|patch|update|modify|replace)\b/.test(text)) return 'edit_files';
  if (/\b(read|open|view|cat)\b/.test(text)) return 'read_files';
  if (/\b(bash|shell|exec|execute|terminal|command|run)\b/.test(text)) return 'run_commands';
  if (/\b(load|loaded)\b.*\btools?\b/.test(text)) return 'load_tools';
  return 'generic';
};

const getToolReceiptTarget = (tool: NormalizedToolCall, action: ToolReceiptAction): string | undefined => {
  if (action === 'run_commands') {
    return getCommandTarget(tool);
  }
  if (action === 'read_files' || action === 'edit_files') {
    return getFileTarget(tool);
  }
  if (action === 'web_search') {
    return getWebSearchTarget(tool);
  }
  if (action === 'web_extract') {
    return getWebExtractTarget(tool);
  }
  if (action !== 'generic') return undefined;
  return formatToolTarget(tool);
};

const getToolReceiptDetailTarget = (tool: NormalizedToolCall, action: ToolReceiptAction): string | undefined => {
  const description = compactToolText(tool.description);
  const name = compactToolText(tool.name);

  if (action === 'generic') return formatToolTarget(tool);
  if (action === 'web_search') return getWebSearchTarget(tool);
  if (action === 'web_extract') return getWebExtractTarget(tool);
  if (action === 'read_files' || action === 'edit_files') return getFileTarget(tool);
  if (description && description !== name) return description;
  if (action === 'run_commands') return getCommandTarget(tool);
  return undefined;
};

const getToolProcessState = (tool: NormalizedToolCall): TurnDisclosureProcessState => {
  if (tool.status === 'running' || tool.status === 'pending') return 'running';
  if (tool.nonFatalFailure) return 'completed';
  if (tool.status === 'error') return 'failed';
  if (tool.status === 'canceled') return 'canceled';
  return 'completed';
};

export const buildToolReceiptSummaryParts = (
  tools: NormalizedToolCall[],
  _state: TurnDisclosureProcessState
): ToolReceiptSummaryPart[] => {
  const grouped = new Map<
    ToolReceiptAction,
    { count: number; skippedCount: number; targets: string[]; states: TurnDisclosureProcessState[] }
  >();

  tools.forEach((tool) => {
    const action = classifyToolForReceipt(tool);
    const target = getToolReceiptTarget(tool, action);
    const current = grouped.get(action) ?? { count: 0, skippedCount: 0, targets: [], states: [] };
    current.count += 1;
    if (tool.skipped) current.skippedCount += 1;
    current.states.push(getToolProcessState(tool));
    if (target) current.targets.push(target);
    grouped.set(action, current);
  });

  return Array.from(grouped.entries()).map(([action, value]) => ({
    action,
    count: value.count,
    state: mergeProcessStates(value.states),
    ...(value.targets.length ? { target: Array.from(new Set(value.targets)).join(', ') } : {}),
    ...(value.skippedCount === value.count ? { skipped: true } : {}),
  }));
};

export const getToolReceiptIconFromSummaryParts = (parts: ToolReceiptSummaryPart[]): ToolReceiptIcon | undefined => {
  const focusedPart =
    parts.findLast((part) => part.state === 'running' || part.state === 'waiting') ??
    parts.findLast((part) => part.state === 'failed' || part.state === 'canceled') ??
    parts.at(-1);
  return focusedPart ? toolReceiptIconByAction[focusedPart.action] : undefined;
};

export const buildToolReceiptDetailRows = (tools: NormalizedToolCall[]): ToolReceiptDetailRow[] =>
  tools.map((tool) => {
    const action = classifyToolForReceipt(tool);
    const title = compactToolText(tool.name) || tool.key;
    const target = getToolReceiptDetailTarget(tool, action);
    return {
      key: tool.key,
      action,
      state: getToolProcessState(tool),
      title,
      ...(target ? { target } : {}),
      ...(tool.input ? { input: tool.input } : {}),
      ...(tool.output ? { output: tool.output } : {}),
      ...(tool.truncated ? { truncated: tool.truncated } : {}),
      ...(tool.skipped ? { skipped: true } : {}),
    };
  });

export const buildToolSummaryDescriptor = (
  tools: NormalizedToolCall[],
  state: TurnDisclosureProcessState
): ToolSummaryDescriptor | null => {
  if (!tools.length) return null;

  const focusedTool = tools.findLast((tool) => stateMatchesTool(state, tool)) ?? tools.at(-1);
  if (!focusedTool) return null;

  return {
    target: formatToolTarget(focusedTool),
    count: tools.length,
  };
};
