/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { ProjectedRequestSummary, ProjectedResponseSummary } from './useAgentTraces';

type Translate = (key: string, options?: Record<string, unknown>) => string;

export function toolTileTitle(t: Translate, tool: { name?: string | null }): string {
  const name = tool.name?.trim();
  return name || t('conversation.agentTrace.tools');
}

function requestOmitted(summary: ProjectedRequestSummary): boolean {
  return Boolean(summary.system_omitted || summary.messages_omitted || summary.tools_omitted);
}

export function requestTileTitle(
  t: Translate,
  summary?: ProjectedRequestSummary | null
): string {
  const model = summary?.model?.trim();
  if (model) return model;
  if (!summary) return t('conversation.agentTrace.previewMissing');
  if (requestOmitted(summary)) return t('conversation.agentTrace.omittedField');
  return t('conversation.agentTrace.requestStage');
}

export function requestTileMeta(
  t: Translate,
  summary?: ProjectedRequestSummary | null
): string {
  if (!summary) return '';
  return t('conversation.agentTrace.requestCounts', {
    system: summary.has_system ? 1 : 0,
    messages: summary.message_count,
    tools: summary.tool_definition_count,
  });
}

export function responseTileCopy(
  t: Translate,
  summary?: ProjectedResponseSummary | null
): { title: string; meta: string } {
  const toolUse = summary?.tool_use_count ?? 0;
  const title = (() => {
    const preview = summary?.text_preview?.trim();
    if (preview) return preview;
    if (summary?.has_text) return t('conversation.agentTrace.partText');
    if (summary?.text_omitted) return t('conversation.agentTrace.omittedField');
    if (summary?.has_thinking) return t('conversation.agentTrace.partThinking');
    if (toolUse > 0) return t('conversation.agentTrace.responseToolsOnly');
    if (summary?.thinking_omitted) return t('conversation.agentTrace.omittedField');
    if (summary) return t('conversation.agentTrace.partText');
    return t('conversation.agentTrace.responseStage');
  })();
  const parts: string[] = [];
  if (summary?.has_thinking) parts.push(t('conversation.agentTrace.partThinking'));
  if (summary?.has_text) parts.push(t('conversation.agentTrace.partText'));
  if (summary?.text_omitted || summary?.thinking_omitted) {
    parts.push(t('conversation.agentTrace.omittedField'));
  }
  if (toolUse > 0) {
    parts.push(t('conversation.agentTrace.partToolUse', { count: toolUse }));
  }
  return { title, meta: parts.join(' · ') };
}

export function gapSeqLabel(
  t: Translate,
  gap: { event_seq: number; from_seq?: number | null; to_seq?: number | null }
): string {
  if (gap.from_seq != null && gap.to_seq != null) {
    return t('conversation.agentTrace.gapSeqRange', {
      seq: gap.event_seq,
      from: gap.from_seq,
      to: gap.to_seq,
    });
  }
  return t('conversation.agentTrace.gapSeq', { seq: gap.event_seq });
}

export function shouldCloseWorkspaceOnEscape(
  event: { key: string; defaultPrevented: boolean },
  overlayOpen: boolean
): boolean {
  return event.key === 'Escape' && !event.defaultPrevented && !overlayOpen;
}

export function sessionLogsOverlayOpen(root: ParentNode | Document | null = document): boolean {
  if (root == null) return false;
  return Boolean(root.querySelector('.arco-modal-wrapper, [role="dialog"]'));
}
