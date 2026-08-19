/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';
import { CallDetailLru, MAX_CALL_DETAIL_CACHE } from './callDetailCache';
import type { ProjectedModelCall } from './useAgentTraces';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('AgentTraceInspector', () => {
  test('gates rendering on system.developerMode and mounts from ChatLayout', () => {
    const inspector = readSource(new URL('./index.tsx', import.meta.url));
    const layout = readSource(new URL('../ChatLayout/index.tsx', import.meta.url));

    expect(inspector.includes("useConfig('system.developerMode')")).toBe(true);
    expect(inspector.includes('if (developerMode !== true)')).toBe(true);
    expect(inspector.includes('return null')).toBe(true);
    expect(inspector.includes('/api/debug/agent-traces')).toBe(false);
    expect(inspector.includes('ObservationWorkflow')).toBe(true);
    expect(inspector.includes('fixed inset-0')).toBe(true);
    expect(inspector.includes('min(760px')).toBe(false);
    expect(layout.includes("from '@/renderer/pages/conversation/components/AgentTraceInspector'")).toBe(true);
    expect(layout.includes('<AgentTraceInspector conversationId={conversation_id} />')).toBe(true);
  });

  test('detail surfaces integrity, gap, interrupted, and REQUEST→RESPONSE→tools', () => {
    const inspector = readSource(new URL('./index.tsx', import.meta.url));
    const workflow = readSource(new URL('./ObservationWorkflow.tsx', import.meta.url));
    expect(inspector.includes('integrity')).toBe(true);
    expect(inspector.includes('interrupted')).toBe(true);
    expect(inspector.includes('writerHealth')).toBe(true);
    expect(inspector.includes('sessionLog')).toBe(true);
    expect(workflow.includes('conversation.agentTrace.request')).toBe(true);
    expect(workflow.includes('conversation.agentTrace.response')).toBe(true);
    expect(workflow.includes('conversation.agentTrace.tools')).toBe(true);
    expect(workflow.includes('conversation.agentTrace.gap')).toBe(true);
    expect(workflow.includes('conversation.agentTrace.interrupted')).toBe(true);
    expect(workflow.includes('copyJson')).toBe(true);
    expect(workflow.includes('root_turn_id')).toBe(true);
    expect(workflow.includes('canonicalRequestFromPayload')).toBe(true);
    expect(workflow.includes('useVirtualizer')).toBe(true);
    expect(workflow.includes('omittedField')).toBe(true);
    expect(workflow.includes('retentionRemoved')).toBe(true);
  });

  test('fetch helpers target session-observations and do not rebuild chat messages', () => {
    const inspector = readSource(new URL('./index.tsx', import.meta.url));
    const hook = readSource(new URL('./useAgentTraces.ts', import.meta.url));
    expect(hook.includes('/api/debug/session-observations?')).toBe(true);
    expect(hook.includes('/api/debug/session-observations/turns/')).toBe(true);
    expect(hook.includes('/calls/')).toBe(true);
    expect(hook.includes('/api/debug/agent-traces')).toBe(false);
    expect(hook.includes('conversation_id')).toBe(true);
    expect(inspector.includes('listSessionObservations')).toBe(true);
    expect(inspector.includes('[...page.turns].reverse()')).toBe(true);
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
    expect(inspector.includes("health.status === 'queue_dropped'")).toBe(false);
    const refreshStart = inspector.indexOf('const refreshWorkspace = useCallback');
    const refreshEnd = inspector.indexOf('const closeWorkspace = useCallback');
    const refreshBody = inspector.slice(refreshStart, refreshEnd);
    expect(refreshBody.includes('callCacheRef.current.get(')).toBe(false);
    expect(refreshBody.includes('getSessionObservationCall')).toBe(true);
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
  });
});
