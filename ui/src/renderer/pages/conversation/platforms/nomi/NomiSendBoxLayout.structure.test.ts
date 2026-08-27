/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('Nomi sendbox control layout', () => {
  test('renders strategy, model, and context slots without turn metrics copy', () => {
    const source = readSource(new URL('./NomiSendBox.tsx', import.meta.url));
    const sendBoxSource = readSource(new URL('../../../../components/chat/SendBox/index.tsx', import.meta.url));
    const contextRingSource = readSource(new URL('./ContextUsageRing.tsx', import.meta.url));
    const useNomiMessageSource = readSource(new URL('./useNomiMessage.ts', import.meta.url));
    const sendBoxIndex = source.indexOf('<SendBox');
    const rightToolsIndex = source.indexOf('rightTools={');
    const modelIndex = source.indexOf('<NomiModelSelector', rightToolsIndex);
    const contextRingIndex = source.indexOf('<ContextUsageRing', rightToolsIndex);

    expect(sendBoxIndex).toBeGreaterThan(-1);
    expect(rightToolsIndex).toBeGreaterThan(sendBoxIndex);
    expect(contextRingIndex).toBeGreaterThan(rightToolsIndex);
    const reasoningIndex = source.indexOf('<ReasoningEffortSelector', rightToolsIndex);
    expect(reasoningIndex).toBeGreaterThan(rightToolsIndex);
    expect(modelIndex).toBeGreaterThan(reasoningIndex);
    expect(contextRingIndex).toBeGreaterThan(modelIndex);
    expect(source.includes('collaboratorSelectorNode')).toBe(false);
    expect(source.includes('topRightTools={')).toBe(false);
    expect(source.includes('ContextUsagePill')).toBe(false);
    expect(source.includes("data-testid='nomi-context-usage-slot'")).toBe(true);
    expect(source.includes("type ActiveChatPopup = 'model' | 'strategy' | 'context' | null")).toBe(true);
    expect(source.includes('data-chat-popup={activeChatPopup ?? undefined}')).toBe(true);
    expect(source.includes("data-testid='nomi-turn-metrics'")).toBe(false);
    expect(source.includes('formatTurnDuration')).toBe(false);
    expect(source.includes('formatTokenCount(tokenUsage.total_tokens)')).toBe(false);
    expect(sendBoxSource.includes("data-testid='sendbox-internal-status-row'")).toBe(true);
    expect(sendBoxSource.includes("data-testid='sendbox-top-right-tools'")).toBe(false);
    expect(contextRingSource.includes("data-testid='nomi-context-usage-ring'")).toBe(true);
    expect(contextRingSource.includes("data-testid='nomi-context-usage-popover'")).toBe(true);
    expect(contextRingSource.includes('getPopupContainer={() => document.body}')).toBe(true);
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
    expect(contextRingSource.includes('inputTokens != null || outputTokens != null || reasoningTokens != null')).toBe(
      true
    );
    expect(contextRingSource.includes('formatTokenCount(outputTokens)')).toBe(true);
    expect(contextRingSource.includes('formatTokenCount(reasoningTokens)')).toBe(true);
    expect(contextRingSource.includes('included in output')).toBe(true);
    expect(useNomiMessageSource.includes('total_tokens: (inputTokens ?? 0) + (outputTokens ?? 0)')).toBe(true);
    expect(contextRingSource.includes("data-testid='nomi-context-usage'")).toBe(false);
    expect(contextRingSource.includes('rd-999px b b-solid px-10px')).toBe(false);
    expect(source.includes('breakdown={tokenUsage?.context_breakdown}')).toBe(true);
    expect(source.includes('catalogContextLimitForModel')).toBe(true);
    expect(source.includes('resolveDisplayContextWindow')).toBe(true);
    expect(source.includes('max={displayContextWindow}')).toBe(true);
  });

  test('does not render collaborator model or collaboration policy controls', () => {
    const chatSource = readSource(new URL('../../components/ChatConversation.tsx', import.meta.url));
    const sendBoxSource = readSource(new URL('./NomiSendBox.tsx', import.meta.url));

    const rightToolsIndex = sendBoxSource.indexOf('rightTools={');
    const contextRingIndex = sendBoxSource.indexOf('<ContextUsageRing', rightToolsIndex);
    const modelIndex = sendBoxSource.indexOf('<NomiModelSelector', rightToolsIndex);

    expect(contextRingIndex).toBeGreaterThan(rightToolsIndex);
    const reasoningIndex = sendBoxSource.indexOf('<ReasoningEffortSelector', rightToolsIndex);
    expect(reasoningIndex).toBeGreaterThan(rightToolsIndex);
    expect(modelIndex).toBeGreaterThan(reasoningIndex);
    expect(contextRingIndex).toBeGreaterThan(modelIndex);
    const toolsIndex = sendBoxSource.indexOf('tools={');
    const modeIndex = sendBoxSource.indexOf('<AgentModeSelector', toolsIndex);
    const fileAttachIndex = sendBoxSource.indexOf('<FileAttachButton', toolsIndex);
    expect(fileAttachIndex).toBeGreaterThan(toolsIndex);
    expect(modeIndex).toBeGreaterThan(fileAttachIndex);
    expect(sendBoxSource.includes('<TaskProfileSelector')).toBe(false);
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
    expect(sendBoxCss.includes('.sendbox-actions--nomi .nomi-context-usage-slot')).toBe(true);
    expect(sendBoxCss.includes('.sendbox-responsive-config-group {\n  --chat-action-slot-size: 28px;')).toBe(true);
    expect(sendBoxCss.includes('.sendbox-responsive-config-group {\n  --chat-action-slot-size: 28px;\n  --chat-action-gap: 6px;')).toBe(true);
    expect(sendBoxCss.includes('display: flex;')).toBe(true);
    expect(sendBoxCss.includes('gap: var(--chat-action-gap);')).toBe(true);
    expect(sendBoxCss.includes('isolation: isolate;')).toBe(true);
    expect(sendBoxSource.includes('chat-model-picker-config-group')).toBe(true);
    expect(sendBoxCss.includes('@container sendbox-config (max-width: 560px)')).toBe(true);
    expect(sendBoxCss.includes('.sendbox-responsive-label')).toBe(true);
    expect(sendBoxCss.includes('max-width 160ms ease')).toBe(false);
    expect(sendBoxCss.includes('@media (hover: hover) and (pointer: fine)')).toBe(true);
    expect(sendBoxCss.includes('.nomi-sendbox-model-btn:hover')).toBe(false);
    expect(sendBoxCss.includes('.chat-model-picker-trigger')).toBe(true);
    expect(sendBoxCss.includes('.chat-model-picker-trigger:hover')).toBe(true);
    expect(sendBoxCss.includes('.chat-model-picker-trigger .flowy-button-inline-content')).toBe(true);
    expect(sendBoxCss.includes('.sendbox-responsive-control-open')).toBe(true);
    expect(sendBoxCss.includes('.chat-model-picker-slot .chat-model-picker-trigger.sendbox-responsive-control-open')).toBe(true);
    expect(sendBoxCss.includes('--chat-model-picker-slot-width: 176px')).toBe(true);
    expect(sendBoxCss.includes('--chat-model-picker-slot-width: 136px')).toBe(true);
    expect(sendBoxCss.includes('flex-basis: var(--chat-model-picker-slot-width) !important;')).toBe(true);
    expect(sendBoxCss.includes('width: var(--chat-model-picker-slot-width) !important;')).toBe(true);
    expect(sendBoxCss.includes('flex-basis: 28px !important;')).toBe(true);
    expect(sendBoxCss.includes('--sendbox-strategy-trigger-width: 108px')).toBe(true);
    expect(sendBoxCss.includes('--sendbox-strategy-label-width: 56px')).toBe(true);
    expect(sendBoxCss.includes(".sendbox-strategy-slot > [data-static='true']")).toBe(true);
    expect(sendBoxCss.includes('sendbox-responsive-strategy-root:not([data-static=\'true\'])')).toBe(true);
    expect(sendBoxCss.includes('position: static !important;')).toBe(true);
    expect(sendBoxCss.includes('justify-content: center !important;')).toBe(true);
    expect(sendBoxCss.includes('overflow: visible !important;')).toBe(true);
    expect(sendBoxCss.includes('flex: 0 0 14px !important;')).toBe(true);
    expect(sendBoxCss.includes('flex-basis: var(--sendbox-strategy-trigger-width) !important;')).toBe(true);
    expect(sendBoxCss.includes('.sendbox-strategy-slot')).toBe(true);
    expect(sendBoxCss.includes('.chat-model-picker-slot')).toBe(true);
    expect(sendBoxCss.includes('position: absolute !important;')).toBe(true);
    expect(sendBoxCss.includes('visibility: hidden;')).toBe(true);
    expect(sendBoxCss.includes('pointer-events: none;')).toBe(true);
    expect(sendBoxCss.includes('margin-inline: 8px')).toBe(false);
    expect(sendBoxCss.includes('.sendbox-responsive-leading-icon')).toBe(true);
    expect(sendBoxCss.includes('.sendbox-responsive-chevron-slot')).toBe(true);
    expect(sendBoxCss.includes('width: 27px !important;')).toBe(true);
    expect(sendBoxCss.includes('width: 100% !important;\n  }\n\n  /* This rule intentionally comes after')).toBe(true);
    expect(sendBoxSource.includes("data-layout-slot='strategy'")).toBe(true);
    expect(sendBoxSource.includes("data-layout-slot='model'")).toBe(true);
    expect(sendBoxSource.includes("data-layout-slot='context'")).toBe(true);
    expect(sendBoxSource.includes("data-testid='nomi-strategy-slot'")).toBe(true);
    expect(sendBoxSource.includes("data-testid='nomi-chat-model-slot'")).toBe(true);
    expect(modelSource.includes('ChatModelPickerMenu')).toBe(true);
    expect(modelSource.includes('conversation.modelPicker.search')).toBe(false);
    expect(modelSource.includes('title={label}')).toBe(true);
    expect(sendBoxCss.includes('.composer-toolbar-tools .agent-mode-compact-pill:hover')).toBe(true);
    expect(arcoOverrideSource.includes(".sendbox-actions [data-testid='nomi-model-selector'].arco-btn:hover")).toBe(true);
    expect(sendBoxCss.includes('display: inline-flex !important')).toBe(true);
    expect(modelSource.includes('flowy-icon-text-btn')).toBe(true);
    expect(modelSource.includes('flowy-button-inline-content')).toBe(true);
    expect(modeSource.includes('flowy-icon-text-btn')).toBe(true);
    expect(modeSource.includes('flowy-button-inline-content')).toBe(true);
    expect(sendBoxCss.includes('.flowy-button-inline-content')).toBe(true);

    for (const source of [modelSource, collaboratorSource, modeSource]) {
      expect(source.includes('<Tooltip')).toBe(false);
      expect(source.includes('sendbox-responsive-label')).toBe(true);
      expect(source.includes('aria-label=')).toBe(true);
    }
  });

  test('leaves model labels complete for the fixed CSS trigger and mobile sheet', () => {
    const selectionSource = readSource(new URL('./useNomiModelSelection.ts', import.meta.url));
    expect(selectionSource.includes('return formatModelLabel(liveCurrentProvider, modelName);')).toBe(true);
    expect(selectionSource.includes('maxLength = 20')).toBe(false);
    expect(selectionSource.includes('slice(0, maxLength)')).toBe(false);
  });

  test('keeps the compact reasoning trigger icon at full size when a narrow sendbox expands on hover', () => {
    const sendBoxCss = readSource(new URL('../../../../components/chat/SendBox/sendbox.css', import.meta.url));
    const guidCss = readSource(new URL('../../../guid/index.module.css', import.meta.url));
    const selectorCss = readSource(new URL('../../../../components/agent/ReasoningEffortSelector.module.css', import.meta.url));
    const selectorSource = readSource(new URL('../../../../components/agent/ReasoningEffortSelector.tsx', import.meta.url));

    expect(selectorSource.includes("classNames(styles.triggerIcon, 'shrink-0')")).toBe(true);
    expect(selectorCss.includes('.compactTrigger > :global(.i-icon):first-child')).toBe(true);
    expect(selectorCss.includes('.triggerIcon')).toBe(true);
    expect(selectorCss.includes('flex: 0 0 auto')).toBe(true);
    expect(selectorCss.includes('transform: translateY(2px)')).toBe(false);

    expect(sendBoxCss.includes('.sendbox-strategy-slot .sendbox-responsive-reasoning-btn')).toBe(true);
    expect(sendBoxCss.includes('.sendbox-responsive-config-group .sendbox-strategy-slot .sendbox-responsive-reasoning-btn:hover')).toBe(true);
    expect(sendBoxCss.includes('.sendbox-responsive-config-group .sendbox-strategy-slot .sendbox-responsive-reasoning-btn:hover .sendbox-responsive-label')).toBe(true);
    expect(sendBoxCss.includes('.sendbox-responsive-config-group .sendbox-strategy-slot .sendbox-responsive-reasoning-btn:focus-visible .sendbox-responsive-label')).toBe(true);
    expect(sendBoxCss.includes('.sendbox-responsive-config-group .sendbox-strategy-slot .sendbox-responsive-reasoning-btn.sendbox-responsive-control-open')).toBe(true);
    expect(sendBoxCss.includes('.sendbox-responsive-config-group .sendbox-strategy-slot .sendbox-responsive-reasoning-btn:hover,')).toBe(true);
    expect(sendBoxCss.includes('.sendbox-responsive-config-group .sendbox-strategy-slot .sendbox-responsive-reasoning-btn:focus-visible,')).toBe(true);
    expect(selectorSource.includes('sendbox-responsive-control-open')).toBe(true);

    expect(guidCss.includes('.actionConfigGroup :global(.sendbox-strategy-slot .sendbox-responsive-reasoning-btn:hover .sendbox-responsive-label)')).toBe(true);
    expect(guidCss.includes('.actionConfigGroup :global(.sendbox-strategy-slot .sendbox-responsive-reasoning-btn:focus-visible .sendbox-responsive-label)')).toBe(true);
    expect(guidCss.includes('.actionConfigGroup :global(.sendbox-strategy-slot .sendbox-responsive-reasoning-btn.sendbox-responsive-control-open)')).toBe(true);
    expect(guidCss.includes('.actionConfigGroup :global(.sendbox-strategy-slot .sendbox-responsive-reasoning-btn:hover)')).toBe(true);
    expect(guidCss.includes('.actionConfigGroup :global(.sendbox-strategy-slot .sendbox-responsive-reasoning-btn:focus-visible)')).toBe(true);
    expect(guidCss.includes('.actionConfigGroup :global(.sendbox-strategy-slot)')).toBe(true);
  });

  test('uses grouped mobile model options with keyboard-accessible sheet rows', () => {
    const nomiSource = readSource(new URL('./NomiSendBox.tsx', import.meta.url));
    const sheetSource = readSource(
      new URL('../../../../components/chat/MobileActionSheet/MobileActionSheet.tsx', import.meta.url),
    );
    const sheetTypes = readSource(new URL('../../../../components/chat/MobileActionSheet/types.ts', import.meta.url));
    const sheetCss = readSource(
      new URL('../../../../components/chat/MobileActionSheet/MobileActionSheet.module.css', import.meta.url),
    );

    expect(nomiSource.includes('type MobileActionSheetOptionGroup')).toBe(true);
    expect(nomiSource.includes('const modelGroups: MobileActionSheetOptionGroup[]')).toBe(true);
    expect(nomiSource.includes("option.family === 'cloud'")).toBe(true);
    expect(nomiSource.includes('otherProviderGroups')).toBe(true);
    expect(nomiSource.includes('groups: modelGroups')).toBe(true);
    expect(nomiSource.includes('const modelOptions: MobileActionSheetOption[]')).toBe(false);
    expect(sheetTypes.includes('groups?: MobileActionSheetOptionGroup[]')).toBe(true);
    expect(sheetSource.includes("role='button'")).toBe(true);
    expect(sheetSource.includes('activateOnKeyboard')).toBe(true);
    expect(sheetSource.includes('aria-pressed')).toBe(true);
    expect(sheetSource.includes("key === 'Escape'")).toBe(true);
    expect(sheetSource.includes("event.key !== 'Tab'")).toBe(true);
    expect(sheetSource.includes('previouslyFocusedRef')).toBe(true);
    expect(sheetCss.includes('.groupHeader')).toBe(true);
    expect(sheetCss.includes('@media (prefers-reduced-motion: reduce)')).toBe(true);
  });
});
