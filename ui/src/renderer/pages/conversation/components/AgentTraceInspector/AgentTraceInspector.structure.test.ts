/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

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
    expect(inspector.includes('min(760px')).toBe(true);
    expect(layout.includes("from '@/renderer/pages/conversation/components/AgentTraceInspector'")).toBe(true);
    expect(layout.includes('<AgentTraceInspector conversationId={conversation_id} />')).toBe(true);
  });

  test('detail surfaces integrity, gap, interrupted, and REQUEST→RESPONSE→tools', () => {
    const inspector = readSource(new URL('./index.tsx', import.meta.url));
    const workflow = readSource(new URL('./ObservationWorkflow.tsx', import.meta.url));
    expect(inspector.includes('integrity')).toBe(true);
    expect(inspector.includes('interrupted')).toBe(true);
    expect(workflow.includes('conversation.agentTrace.request')).toBe(true);
    expect(workflow.includes('conversation.agentTrace.response')).toBe(true);
    expect(workflow.includes('conversation.agentTrace.tools')).toBe(true);
    expect(workflow.includes('conversation.agentTrace.gap')).toBe(true);
    expect(workflow.includes('conversation.agentTrace.interrupted')).toBe(true);
    expect(workflow.includes('copyJson')).toBe(true);
    expect(workflow.includes('root_turn_id')).toBe(true);
    expect(workflow.includes('canonicalRequestFromPayload')).toBe(true);
  });

  test('fetch helpers target session-observations and do not rebuild chat messages', () => {
    const inspector = readSource(new URL('./index.tsx', import.meta.url));
    const hook = readSource(new URL('./useAgentTraces.ts', import.meta.url));
    expect(hook.includes('/api/debug/session-observations?')).toBe(true);
    expect(hook.includes('/api/debug/session-observations/turns/')).toBe(true);
    expect(hook.includes('/api/debug/agent-traces')).toBe(false);
    expect(hook.includes('conversation_id')).toBe(true);
    expect(inspector.includes('listSessionObservations')).toBe(true);
    expect(inspector.includes('[...rows].reverse()')).toBe(true);
    expect(inspector.includes('ordered[0].root_turn_id')).toBe(true);
    expect(inspector.includes('getSessionObservationTurn')).toBe(true);
    expect(inspector.includes('persisted observation projections')).toBe(true);
    expect(hook.includes('canonicalRequestFromPayload')).toBe(true);
    expect(hook.includes('Never invent omitted fields')).toBe(true);
  });
});
