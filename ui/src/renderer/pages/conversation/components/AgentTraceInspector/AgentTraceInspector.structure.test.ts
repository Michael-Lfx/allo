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
    expect(inspector.includes('/api/debug/agent-traces')).toBe(false); // hook owns the path
    expect(inspector.includes('TraceTurnSummary')).toBe(true);
    expect(inspector.includes('min(760px')).toBe(true);
    expect(layout.includes("from '@/renderer/pages/conversation/components/AgentTraceInspector'")).toBe(true);
    expect(layout.includes('<AgentTraceInspector conversationId={conversation_id} />')).toBe(true);
  });

  test('detail surfaces copyable identity and expandable span attributes', () => {
    const summary = readSource(new URL('./TraceTurnSummary.tsx', import.meta.url));
    const timeline = readSource(new URL('./TraceTimeline.tsx', import.meta.url));
    expect(summary.includes('copyJson')).toBe(true);
    expect(summary.includes('trace_id')).toBe(true);
    expect(summary.includes('cache_read_tokens')).toBe(true);
    expect(timeline.includes('attributes')).toBe(true);
    expect(timeline.includes('Collapse')).toBe(true);
    expect(timeline.includes('preview')).toBe(true);
    expect(timeline.includes('searchSpans')).toBe(true);
    expect(timeline.includes('copySpan')).toBe(true);
  });

  test('fetch helper targets debug agent-traces endpoints', () => {
    const hook = readSource(new URL('./useAgentTraces.ts', import.meta.url));
    expect(hook.includes('/api/debug/agent-traces?')).toBe(true);
    expect(hook.includes('/api/debug/agent-traces/${encodeURIComponent(traceId)}')).toBe(true);
    expect(hook.includes('conversation_id')).toBe(true);
  });
});
