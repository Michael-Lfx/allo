/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';
import { CallDetailLru, MAX_CALL_DETAIL_CACHE } from './callDetailCache';
import { assignTurnRounds, formatClock, formatDurationMs } from './format';
import type { ProjectedModelCall } from './useAgentTraces';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('AgentTraceInspector', () => {
  test('gates rendering on system.developerMode and mounts from ChatLayout', () => {
    const inspector = readSource(new URL('./index.tsx', import.meta.url));
    const layout = readSource(new URL('../ChatLayout/index.tsx', import.meta.url));
    const layoutCss = readSource(new URL('../ChatLayout/chat-layout.css', import.meta.url));

    expect(inspector.includes("useConfig('system.developerMode')")).toBe(true);
    expect(inspector.includes('if (developerMode !== true)')).toBe(true);
    expect(inspector.includes('return null')).toBe(true);
    expect(inspector.includes('/api/debug/agent-traces')).toBe(false);
    expect(inspector.includes('ObservationWorkflow')).toBe(true);
    expect(inspector.includes('createPortal')).toBe(false);
    expect(inspector.includes('document.body')).toBe(false);
    expect(inspector.includes('fixed inset-0')).toBe(false);
    expect(inspector.includes('z-[10000]')).toBe(false);
    expect(inspector.includes('min(760px')).toBe(false);
    expect(layout.includes("from '@/renderer/pages/conversation/components/AgentTraceInspector'")).toBe(true);
    expect(layout.includes('<SessionLogsRoot')).toBe(true);
    expect(layout.includes('<AgentTraceTrigger />')).toBe(true);
    expect(layout.includes('<SessionLogWorkspace />')).toBe(true);
    expect(layout.includes('conversation-column-track')).toBe(true);
    expect(layout.includes('tabDialogue')).toBe(false);
    expect(layout.includes('tabLogs')).toBe(false);
    expect(layout.includes('conversation-column-tabs')).toBe(false);
    expect(layout.includes('hideAdvancedControls')).toBe(true);
    expect(layoutCss.includes('transform: translateX')).toBe(true);
    expect(layoutCss.includes('--ease-out-expo')).toBe(true);
    expect(layoutCss.includes('prefers-reduced-motion')).toBe(true);
    expect(layoutCss.includes('container-type: inline-size')).toBe(true);

    const triggerIndex = layout.indexOf('{logsEnabled ? <AgentTraceTrigger /> : null}');
    const knowledgeIndex = layout.indexOf('<KnowledgeControl');
    expect(triggerIndex).toBeGreaterThan(-1);
    expect(knowledgeIndex).toBeGreaterThan(-1);
    expect(triggerIndex).toBeLessThan(knowledgeIndex);
  });

  test('observe pill and nav chrome replace column tabs', () => {
    const inspector = readSource(new URL('./index.tsx', import.meta.url));
    const css = readSource(new URL('./session-logs.css', import.meta.url));
    expect(inspector.includes('aria-pressed={open}')).toBe(true);
    expect(inspector.includes('newestFirst')).toBe(true);
    expect(inspector.includes('oldestFirst')).toBe(true);
    expect(inspector.includes('SortAmountDown')).toBe(true);
    expect(inspector.includes('SortAmountUp')).toBe(true);
    expect(inspector.includes('collapseRoundList')).toBe(true);
    expect(inspector.includes('expandRoundList')).toBe(true);
    expect(inspector.includes('navCollapsed')).toBe(true);
    expect(inspector.includes('ExpandLeft')).toBe(true);
    expect(css.includes('.session-logs-nav.is-collapsed')).toBe(true);
    expect(inspector.includes('Popover')).toBe(true);
    expect(inspector.includes('glossaryAria')).toBe(true);
    expect(inspector.includes('session-logs-glossary')).toBe(true);
    expect(inspector.includes('session-logs-glossary__item')).toBe(true);
    expect(inspector.includes('metricTurnHint')).toBe(true);
    expect(inspector.includes('metricModelHint')).toBe(true);
    expect(inspector.includes('metricToolHint')).toBe(true);
    expect(inspector.includes('metricDurationHint')).toBe(true);
    expect(inspector.includes('integrityHint')).toBe(true);
    expect(inspector.includes('glossaryNotes')).toBe(true);
    expect(inspector.includes('notesHint')).toBe(true);
    expect(inspector.includes('session-logs-integrity')).toBe(false);
    expect(inspector.includes("hint={t('conversation.agentTrace.metricTurnHint')}")).toBe(false);
    expect(inspector.includes('userTurnCount')).toBe(false);
    expect(inspector.includes('formatClock(entry.started_at_ms)')).toBe(true);
    expect(inspector.includes('assignTurnRounds')).toBe(true);
    expect(inspector.includes('shouldCloseWorkspaceOnEscape')).toBe(true);
    expect(inspector.includes('sessionLogsOverlayOpen')).toBe(true);
    expect(inspector.includes('selectedTurnEnded')).toBe(true);
    expect(inspector.includes("if (event.key === 'Escape') closeWorkspace()")).toBe(false);
    expect(inspector.includes("icon={<Close")).toBe(false);
    expect(inspector.includes('Sliding back to dialogue must not abort poll')).toBe(true);
    expect(inspector.includes('capabilityHeaderButtonStyle(OBSERVE_ACCENT)')).toBe(true);
    expect(inspector.includes("capabilityHeaderButtonStyle(ACCENT)")).toBe(false);
    expect(css.includes('.session-logs-nav__item.is-active')).toBe(true);
    expect(css.includes('rgb(var(--primary-6)) 8%')).toBe(false);
    expect(css.includes('rgb(var(--primary-6)) 12%')).toBe(true);
    expect(css.includes('rgb(var(--primary-6)) 16%')).toBe(true);
    expect(css.includes('.session-logs-nav__item:not(:last-child):not(.is-active)')).toBe(true);
    expect(/\.session-logs-nav__list\s*\{[^}]*gap: 8px/s.test(css)).toBe(true);
    expect(css.includes('container-name: session-logs')).toBe(true);
    expect(css.includes('session-logs-nav__list')).toBe(true);
    expect(css.includes('@container session-logs (max-width: 720px)')).toBe(true);
    expect(css.includes('--session-logs-gutter: 16px')).toBe(true);
    expect(css.includes('padding: 12px var(--session-logs-gutter)')).toBe(true);
    expect(css.includes('session-logs-order')).toBe(false);
  });

  test('detail surfaces integrity, gap, interrupted, and model-call request→response→tool flow', () => {
    const inspector = readSource(new URL('./index.tsx', import.meta.url));
    const workflow = readSource(new URL('./ObservationWorkflow.tsx', import.meta.url));
    const tree = readSource(new URL('./ObservationJsonTree.tsx', import.meta.url));
    expect(inspector.includes('integrity')).toBe(true);
    expect(inspector.includes('interrupted')).toBe(true);
    expect(inspector.includes('writerHealth')).toBe(true);
    expect(inspector.includes('sessionLog')).toBe(true);
    expect(inspector.includes('coverageRetained')).toBe(false);
    expect(inspector.includes("summary?.integrity === 'degraded'")).toBe(true);
    expect(inspector.includes('roundLabel')).toBe(true);
    expect(inspector.includes('modelCallCount')).toBe(true);
    expect(inspector.includes('msg=')).toBe(false);
    expect(inspector.includes('title={entry.root_turn_id}')).toBe(false);
    expect(inspector.includes('turn={shortId')).toBe(false);
    expect(workflow.includes('conversation.agentTrace.requestStage')).toBe(true);
    expect(workflow.includes('conversation.agentTrace.responseStage')).toBe(true);
    expect(workflow.includes('conversation.agentTrace.toolStage')).toBe(true);
    expect(workflow.includes('conversation.agentTrace.modelCallLabel')).toBe(true);
    expect(workflow.includes('conversation.agentTrace.finalReply')).toBe(true);
    expect(workflow.includes('conversation.agentTrace.gap')).toBe(true);
    expect(workflow.includes('gapSeq')).toBe(true);
    expect(workflow.includes('seq=')).toBe(false);
    expect(workflow.includes('from=')).toBe(false);
    expect(workflow.includes('tool.tool_call_id || \'anon\'')).toBe(true);
    expect(workflow.includes('requestTileTitle')).toBe(true);
    expect(workflow.includes('requestTileMeta')).toBe(true);
    expect(workflow.includes('responseTileCopy')).toBe(true);
    expect(workflow.includes('conversation.agentTrace.interrupted')).toBe(true);
    expect(workflow.includes("t('conversation.agentTrace.copyJson')")).toBe(false);
    expect(workflow.includes('copyTurn')).toBe(false);
    expect(workflow.includes('copyField')).toBe(true);
    expect(workflow.includes('identityFields')).toBe(true);
    expect(workflow.includes('<Collapse')).toBe(false);
    expect(workflow.includes('copyable={false}')).toBe(true);
    expect(workflow.includes('session-logs-flow__end')).toBe(true);
    expect(workflow.includes('session-logs-flow__end-maximize')).toBe(true);
    expect(workflow.includes('maximizeInspector')).toBe(true);
    expect(workflow.includes("stage: 'final'")).toBe(true);
    expect(workflow.includes('staticTile')).toBe(false);
    expect(workflow.includes('aria-expanded={selected}')).toBe(true);
    expect(workflow.includes('argument_preview')).toBe(true);
    expect(workflow.includes('root_turn_id')).toBe(true);
    expect(workflow.includes('canonicalRequestFromPayload')).toBe(true);
    expect(workflow.includes('useVirtualizer')).toBe(true);
    expect(workflow.includes('omittedField')).toBe(true);
    expect(workflow.includes('retentionRemoved')).toBe(true);
    expect(workflow.includes('inspectSystem')).toBe(true);
    expect(workflow.includes('inspectSystemHint')).toBe(true);
    expect(workflow.includes('inspectReasoning')).toBe(true);
    expect(workflow.includes('inspectRequestTitle')).toBe(true);
    const requestInspector = workflow.slice(
      workflow.indexOf('function requestParamsFromBody'),
      workflow.indexOf('function ResponseInspector'),
    );
    expect(requestInspector.includes('inspectRequestParams')).toBe(true);
    expect(requestInspector.includes('max_tokens')).toBe(true);
    expect(requestInspector.includes('temperature')).toBe(true);
    expect(requestInspector.includes('reasoning_effort')).toBe(true);
    expect(requestInspector.includes('thinking')).toBe(true);
    expect(requestInspector.includes('startCollapsed')).toBe(false);
    expect(requestInspector.includes('input_schema')).toBe(false);
    expect(requestInspector.includes('与上次相同')).toBe(false);
    const systemAt = requestInspector.indexOf("inspectSystem'");
    const gridAt = requestInspector.indexOf("session-logs-inspector__grid");
    expect(systemAt).toBeGreaterThan(gridAt);
    expect(tree.includes('startCollapsed')).toBe(false);
    expect(workflow.includes("JsonBlock label={t('conversation.agentTrace.request')}")).toBe(false);
    expect(workflow.includes("JsonBlock label={t('conversation.agentTrace.response')}")).toBe(false);
    expect(tree.includes("from 'react-json-view-lite'")).toBe(true);
    expect(tree.includes('shouldExpandObservationNode')).toBe(true);
    expect(tree.includes("field === 'messages'")).toBe(true);
    expect(tree.includes("field === 'tools'")).toBe(true);
    expect(tree.includes('forwardPunctuationToggle')).toBe(true);
    expect(tree.includes(':scope > [role="button"]')).toBe(true);
    expect(tree.includes('session-logs-json-tree__punct')).toBe(true);
    expect(tree.includes('session-logs-json-tree__expander')).toBe(true);
    expect(tree.includes('session-logs-json-tree__expander--closed')).toBe(true);
    expect(tree.includes('${defaultStyles.container}')).toBe(false);
    expect(tree.includes("container: 'session-logs-json-tree__container'")).toBe(true);
    expect(tree.includes('session-logs-json-tree__collapsed')).toBe(true);
    expect(tree.includes('copyField')).toBe(true);
    expect(tree.includes('hint?: string')).toBe(true);
    const css = readSource(new URL('./session-logs.css', import.meta.url));
    expect(css.includes('border: 1px solid var(--color-border-2)')).toBe(true);
    expect(css.includes('session-logs-json-tree__expander')).toBe(true);
    const jsonContainer = css.match(/^\.session-logs-json-tree__container\s*\{[^}]+\}/m)?.[0] ?? '';
    expect(jsonContainer.includes('background: transparent')).toBe(true);
    expect(jsonContainer.includes('color-text-1')).toBe(true);
    expect(css.includes('button.session-logs-tile:hover')).toBe(true);
    expect(css.includes('session-logs-flow__end')).toBe(true);
    const flowArrow = css.match(/\.session-logs-flow__arrow\s*\{[^}]+\}/)?.[0] ?? '';
    expect(flowArrow.includes('rgb(var(--primary-6))')).toBe(true);
    expect(flowArrow.includes('color-text-')).toBe(false);
    expect(css.includes('height: 26px')).toBe(true);
    expect(css.includes('flex-direction: column')).toBe(true);
    expect(css.includes('appearance: none')).toBe(true);
    const endLabel = css.match(/\.session-logs-flow__end-label\s*\{[^}]+\}/)?.[0] ?? '';
    expect(endLabel.includes('color-fill-1')).toBe(true);
    expect(endLabel.includes('color-fill-2')).toBe(false);
    expect(endLabel.includes('color-text-2')).toBe(true);
    const endTitle = css.match(/\.session-logs-flow__end-title\s*\{[^}]+\}/)?.[0] ?? '';
    expect(endTitle.includes('color-text-1')).toBe(true);
    expect(endTitle.includes('color-text-3')).toBe(false);
    expect(css.includes('.session-logs-tile--request .session-logs-tile__head')).toBe(true);
    expect(css.includes('.session-logs-tile--response .session-logs-tile__head')).toBe(true);
    expect(css.includes('.session-logs-tile--tool .session-logs-tile__head')).toBe(true);
    const workHeads = css.match(
      /\.session-logs-tile--request \.session-logs-tile__head,\s*\.session-logs-tile--response \.session-logs-tile__head,\s*\.session-logs-tile--tool \.session-logs-tile__head\s*\{[^}]+\}/,
    )?.[0] ?? '';
    expect(workHeads.includes('rgb(var(--primary-6)) 28%')).toBe(true);
    expect(workHeads.includes('rgb(var(--primary-6)) 14%')).toBe(false);
    expect(workHeads.includes('color-fill-2')).toBe(false);
    expect(workHeads.includes('color-fill-3')).toBe(false);
    expect(css.includes('.session-logs-json-tree:hover')).toBe(false);
    expect(workflow.includes('session-logs-inspector-slot')).toBe(true);
    expect(workflow.includes('useOpenTransition')).toBe(true);
    expect(css.includes('scrollbar-gutter: stable')).toBe(true);
    expect(css.includes('height: 320px')).toBe(true);
    expect(css.includes('max-height: 320px')).toBe(false);
    expect(css.includes('grid-template-rows: 0fr')).toBe(true);
    expect(css.includes('@media (prefers-reduced-motion: reduce)')).toBe(true);
    expect(css.includes('session-logs-request-params')).toBe(true);
    expect(css.includes('session-logs-request-params__lead')).toBe(true);
    expect(css.includes('display: contents')).toBe(true);
    const jsonToolbar = css.match(/^\.session-logs-json-tree__toolbar\s*\{[^}]+\}/m)?.[0] ?? '';
    expect(jsonToolbar.includes('rgb(var(--primary-6)) 12%')).toBe(true);
    expect(jsonToolbar.includes('color-fill-2')).toBe(false);
  });

  test('fetch helpers target session-observations and do not rebuild chat messages', () => {
    const inspector = readSource(new URL('./index.tsx', import.meta.url));
    const hook = readSource(new URL('./useAgentTraces.ts', import.meta.url));
    expect(hook.includes('/api/debug/session-observations?')).toBe(true);
    expect(hook.includes('/api/debug/session-observations/turns/')).toBe(true);
    expect(hook.includes('/calls/')).toBe(true);
    expect(hook.includes('/api/debug/agent-traces')).toBe(false);
    expect(hook.includes('conversation_id')).toBe(true);
    expect(hook.includes('ProjectedRequestSummary')).toBe(true);
    expect(hook.includes('ProjectedResponseSummary')).toBe(true);
    expect(hook.includes('argument_preview')).toBe(true);
    expect(inspector.includes('listSessionObservations')).toBe(true);
    expect(inspector.includes('[...entries].reverse()')).toBe(true);
    expect(inspector.includes('refreshWorkspace')).toBe(true);
    expect(inspector.includes('void loadList()')).toBe(false);
    expect(inspector.includes('onClick={() => void refreshWorkspace({ showListLoading: true })}')).toBe(
      true
    );
    expect(inspector.includes('listSeqRef')).toBe(true);
    expect(inspector.includes('turnSeqRef')).toBe(true);
    expect(inspector.includes('callSeqRef')).toBe(true);
    expect(inspector.includes('refreshWorkspace({ signal: controller.signal })')).toBe(true);
    expect(inspector.includes('getSessionObservationTurn')).toBe(true);
    expect(inspector.includes('getSessionObservationCall')).toBe(true);
    expect(inspector.includes("setDetailErrorKey('loadFailed')")).toBe(true);
    expect(inspector.includes('persisted observation projections')).toBe(true);
    expect(hook.includes('canonicalRequestFromPayload')).toBe(true);
    expect(hook.includes('Never invent omitted fields')).toBe(true);
    expect(hook.includes('AbortSignal')).toBe(true);
    expect(hook.includes('if (status === 410) return true')).toBe(false);
  });

  test('refreshWorkspace reloads list turn and call with independent seqs', () => {
    const inspector = readSource(new URL('./index.tsx', import.meta.url));
    expect(inspector.includes('const refreshWorkspace = useCallback')).toBe(true);
    expect(inspector.includes('void loadList()')).toBe(false);
    expect(inspector.includes('listSeqRef')).toBe(true);
    expect(inspector.includes('turnSeqRef')).toBe(true);
    expect(inspector.includes('callSeqRef')).toBe(true);
    expect(inspector.includes('refreshWorkspace({ signal: controller.signal })')).toBe(true);
    expect(inspector.includes('[applyList, conversationId, developerMode, entries, expandedCallId, health, open, selectedId]')).toBe(
      false
    );
    expect(inspector.includes('nextSelected !== previousSelected')).toBe(true);
    expect(inspector.includes('selectedChanged')).toBe(true);
    expect(inspector.includes('nextSelected && !selectedChanged')).toBe(true);
    const applyStart = inspector.indexOf('const applyList = useCallback');
    const applyEnd = inspector.indexOf('const abortWorkspaceFetches');
    const applyBody = inspector.slice(applyStart, applyEnd);
    expect(applyBody.includes('if (nextSelected == null || selectedChanged)')).toBe(true);
    expect(applyBody.includes('setDetail(null)')).toBe(true);
    expect(inspector.includes("health.status === 'queue_dropped'")).toBe(false);
    expect(inspector.includes('callCacheRef.current.delete(')).toBe(true);
    expect(inspector.includes('setEntries([])')).toBe(true);
    const refreshStart = inspector.indexOf('const refreshWorkspace = useCallback');
    const refreshEnd = inspector.indexOf('const closeWorkspace = useCallback');
    const refreshBody = inspector.slice(refreshStart, refreshEnd);
    expect(refreshBody.includes('callCacheRef.current.get(')).toBe(false);
    expect(refreshBody.includes('getSessionObservationCall')).toBe(true);
    expect(refreshBody.includes('setEntries([])')).toBe(false);
    expect(refreshBody.includes('setSelectedIdState(null)')).toBe(false);
    expect(inspector.includes("onViewChange('dialogue')")).toBe(true);
  });

  test('call detail cache keeps two most recent entries', () => {
    const cache = new CallDetailLru();
    expect(MAX_CALL_DETAIL_CACHE).toBe(2);
    const stub = (id: string): ProjectedModelCall => ({
      model_call_id: id,
      interrupted: false,
      tools: [],
    });
    cache.set('a', stub('a'));
    cache.set('b', stub('b'));
    cache.set('c', stub('c'));
    expect(cache.size).toBe(2);
    expect(cache.get('a')).toBeUndefined();
    expect(cache.get('b')?.model_call_id).toBe('b');
    expect(cache.get('c')?.model_call_id).toBe('c');
    cache.delete('b');
    expect(cache.get('b')).toBeUndefined();
    expect(cache.size).toBe(1);
  });

  test('formatDurationMs uses seconds and minutes, not raw milliseconds', () => {
    expect(formatDurationMs(2500)).toBe('2.5s');
    expect(formatDurationMs(65730)).toBe('1m6s');
    expect(formatDurationMs(65730).includes('ms')).toBe(false);
  });

  test('formatClock and round numbers stay stable across display order', () => {
    expect(formatClock(1_700_000_000_000).length).toBeGreaterThan(0);
    const rounds = assignTurnRounds([
      { root_turn_id: 'b', started_at_ms: 200 },
      { root_turn_id: 'a', started_at_ms: 100 },
      { root_turn_id: 'c', started_at_ms: 300 },
    ]);
    expect(rounds.get('a')).toBe(1);
    expect(rounds.get('b')).toBe(2);
    expect(rounds.get('c')).toBe(3);
  });
});
