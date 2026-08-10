/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('Nomi sendbox control layout', () => {
  test('renders context usage as a click ring before the model selector and removes turn metrics copy', () => {
    const source = readSource(new URL('./NomiSendBox.tsx', import.meta.url));
    const sendBoxSource = readSource(new URL('../../../../components/chat/SendBox/index.tsx', import.meta.url));
    const contextRingSource = readSource(new URL('./ContextUsageRing.tsx', import.meta.url));
    const sendBoxIndex = source.indexOf('<SendBox');
    const rightToolsIndex = source.indexOf('rightTools={');
    const modelIndex = source.indexOf('<NomiModelSelector', rightToolsIndex);
    const contextRingIndex = source.indexOf('<ContextUsageRing', rightToolsIndex);

    expect(sendBoxIndex).toBeGreaterThan(-1);
    expect(rightToolsIndex).toBeGreaterThan(sendBoxIndex);
    expect(contextRingIndex).toBeGreaterThan(rightToolsIndex);
    expect(modelIndex).toBeGreaterThan(contextRingIndex);
    const reasoningIndex = source.indexOf('<ReasoningEffortSelector', rightToolsIndex);
    expect(reasoningIndex).toBeGreaterThan(contextRingIndex);
    expect(modelIndex).toBeGreaterThan(reasoningIndex);
    expect(source.includes('collaboratorSelectorNode')).toBe(false);
    expect(source.includes('topRightTools={')).toBe(false);
    expect(source.includes('ContextUsagePill')).toBe(false);
    expect(source.includes("data-testid='nomi-context-usage-slot'")).toBe(false);
    expect(source.includes("data-testid='nomi-turn-metrics'")).toBe(false);
    expect(source.includes('formatTurnDuration')).toBe(false);
    expect(source.includes('formatTokenCount(tokenUsage.total_tokens)')).toBe(false);
    expect(sendBoxSource.includes("data-testid='sendbox-internal-status-row'")).toBe(true);
    expect(sendBoxSource.includes("data-testid='sendbox-top-right-tools'")).toBe(false);
    expect(contextRingSource.includes("data-testid='nomi-context-usage-ring'")).toBe(true);
    expect(contextRingSource.includes("data-testid='nomi-context-usage-popover'")).toBe(true);
    expect(contextRingSource.includes("data-testid='nomi-context-usage-bar'")).toBe(true);
    expect(contextRingSource.includes("data-testid='nomi-context-usage-summarized-props'")).toBe(true);
    expect(contextRingSource.includes("trigger='click'")).toBe(true);
    expect(contextRingSource.includes('conic-gradient')).toBe(true);
    expect(contextRingSource.includes('h-22px w-22px')).toBe(true);
    expect(contextRingSource.includes('w-320px')).toBe(true);
    expect(contextRingSource.includes('formatContextTokenAbbrev')).toBe(true);
    expect(contextRingSource.includes('buildContextBreakdownViewModel')).toBe(true);
    expect(contextRingSource.includes('percentFull')).toBe(true);
    expect(contextRingSource.includes('tokenSummary')).toBe(true);
    expect(contextRingSource.includes('System prompt')).toBe(true);
    expect(contextRingSource.includes('Tool definitions')).toBe(true);
    expect(contextRingSource.includes('MCP & dynamic tools')).toBe(true);
    expect(contextRingSource.includes('Summarized conversation')).toBe(true);
    expect(contextRingSource.includes("data-testid='nomi-context-usage'")).toBe(false);
    expect(contextRingSource.includes('rd-999px b b-solid px-10px')).toBe(false);
    expect(source.includes('breakdown={tokenUsage?.context_breakdown}')).toBe(true);
  });

  test('does not render collaborator model or collaboration policy controls', () => {
    const chatSource = readSource(new URL('../../components/ChatConversation.tsx', import.meta.url));
    const sendBoxSource = readSource(new URL('./NomiSendBox.tsx', import.meta.url));

    const rightToolsIndex = sendBoxSource.indexOf('rightTools={');
    const contextRingIndex = sendBoxSource.indexOf('<ContextUsageRing', rightToolsIndex);
    const modelIndex = sendBoxSource.indexOf('<NomiModelSelector', rightToolsIndex);

    expect(contextRingIndex).toBeGreaterThan(rightToolsIndex);
    expect(modelIndex).toBeGreaterThan(contextRingIndex);
    const toolsIndex = sendBoxSource.indexOf('tools={');
    const modeIndex = sendBoxSource.indexOf('<AgentModeSelector', toolsIndex);
    const fileAttachIndex = sendBoxSource.indexOf('<FileAttachButton', toolsIndex);
    expect(fileAttachIndex).toBeGreaterThan(toolsIndex);
    expect(modeIndex).toBeGreaterThan(fileAttachIndex);
    expect(chatSource.includes('GuidCollaboratorSelector')).toBe(false);
    expect(chatSource.includes('CollaborationPolicyControl')).toBe(false);
    expect(sendBoxSource.includes('collaboratorSelectorNode')).toBe(false);
    expect(sendBoxSource.includes('extraRightTools')).toBe(false);
  });

  test('reconciles conversation collaborators before rendering or persisting executable ranges', () => {
    const chatSource = readSource(new URL('../../components/ChatConversation.tsx', import.meta.url));

    expect(chatSource.includes('import { reconcileModelRefs, sameModelRefs }')).toBe(true);
    expect(chatSource.includes('const activeCollaborators = collaboratorReconciliation?.active ?? []')).toBe(true);
    expect(
      /buildConversationModelPool\(\s*\{ provider_id: _provider\.id, model: modelName \},\s*activeCollaborators,\s*\)/.test(
        chatSource,
      ),
    ).toBe(true);
    expect(chatSource.includes('collaboratorReconciliation.removed.length === 0')).toBe(true);
    expect(chatSource.includes('sameModelRefs(collaborators, collaboratorReconciliation.retained)')).toBe(true);
  });

  test('keeps the compact collaboration policy icon ready for inline hover expansion', () => {
    const source = readSource(
      new URL('../../../../components/collaboration/CollaborationPolicyControl.tsx', import.meta.url),
    );

    expect(source.includes("data-testid='collaboration-policy-control'")).toBe(true);
    expect(source.includes("shape={compact ? 'circle' : 'round'}")).toBe(true);
    expect(source.includes("compact ? 'nomi-sendbox-policy-btn' : ''")).toBe(true);
    expect(source.includes("className='sendbox-responsive-label'")).toBe(true);
    expect(/\{compact && active &&\s*<span className=\{styles\.triggerStatus\}/.test(source)).toBe(true);
  });

  test('collapses text pills to icons and expands their labels inline on desktop hover', () => {
    const sendBoxSource = readSource(new URL('./NomiSendBox.tsx', import.meta.url));
    const composerSource = readSource(new URL('../../../../components/chat/SendBox/index.tsx', import.meta.url));
    const modelSource = readSource(new URL('./NomiModelSelector.tsx', import.meta.url));
    const sendBoxCss = readSource(new URL('../../../../components/chat/SendBox/sendbox.css', import.meta.url));
    const arcoOverrideSource = readSource(new URL('../../../../styles/arco-override.css', import.meta.url));
    const collaboratorSource = readSource(new URL('../../../guid/components/GuidCollaboratorSelector.tsx', import.meta.url));
    const modeSource = readSource(new URL('../../../../components/agent/AgentModeSelector.tsx', import.meta.url));

    expect(sendBoxSource.includes('sendbox-responsive-config-group')).toBe(true);
    expect(sendBoxCss.includes('container-name: sendbox-config')).toBe(true);
    expect(composerSource.includes("'sendbox-actions--nomi' : 'gap-2'")).toBe(true);
    expect(sendBoxCss.includes(".sendbox-actions--nomi [data-testid='nomi-context-usage-ring']")).toBe(true);
    expect(sendBoxCss.includes('.sendbox-responsive-config-group {\n  /* Flatten model controls')).toBe(true);
    expect(sendBoxCss.includes('display: contents;')).toBe(true);
    expect(sendBoxSource.includes("className='sendbox-responsive-config-group'")).toBe(true);
    expect(sendBoxCss.includes('@container sendbox-config (max-width: 560px)')).toBe(true);
    expect(sendBoxCss.includes('.sendbox-responsive-label')).toBe(true);
    expect(sendBoxCss.includes('max-width 160ms ease')).toBe(true);
    expect(sendBoxCss.includes('@media (hover: hover) and (pointer: fine)')).toBe(true);
    expect(sendBoxCss.includes('.nomi-sendbox-model-btn:hover')).toBe(true);
    expect(sendBoxCss.includes('.composer-toolbar-tools .agent-mode-compact-pill:hover')).toBe(true);
    expect(arcoOverrideSource.includes(".sendbox-actions [data-testid='nomi-model-selector'].arco-btn:hover")).toBe(true);
    expect(sendBoxCss.includes('display: inline-flex !important')).toBe(true);

    for (const source of [modelSource, collaboratorSource, modeSource]) {
      expect(source.includes('<Tooltip')).toBe(false);
      expect(source.includes('sendbox-responsive-label')).toBe(true);
      expect(source.includes('aria-label=')).toBe(true);
    }
  });
});
