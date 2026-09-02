/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import {
  buildTimelineRows,
  findTimelineEventSeq,
  timelineRowIconKind,
  timelineRowRelativeTime,
  timelineWaitDurations,
} from './ObservationTimeline';
import type {
  ObservationTimelineEvent,
  ProjectedModelCall,
  ProjectedToolExecution,
} from './useAgentTraces';

function event(
  eventSeq: number,
  eventType: string,
  relativeMs: number,
  extra: Partial<ObservationTimelineEvent> = {},
): ObservationTimelineEvent {
  return {
    event_seq: eventSeq,
    event_type: eventType,
    timestamp_ms: relativeMs,
    relative_ms: relativeMs,
    ...extra,
  };
}

function tool(toolCallId: string, name: string): ProjectedToolExecution {
  return {
    tool_call_id: toolCallId,
    name,
    status: 'completed',
    started_at_ms: 100,
    ended_at_ms: 250,
  };
}

function call(
  modelCallId: string,
  tools: ProjectedToolExecution[] = [],
): ProjectedModelCall {
  return {
    model_call_id: modelCallId,
    interrupted: false,
    tools,
    response_summary: {
      has_text: tools.length === 0,
      has_thinking: false,
      tool_use_count: tools.length,
    },
  };
}

describe('ObservationTimeline projection', () => {
  test('sorts by event sequence and keeps request, response, tool, and next request order', () => {
    const rows = buildTimelineRows(
      [
        event(6, 'llm/response', 700, { model_call_id: 'call-2' }),
        event(5, 'llm/request', 500, { model_call_id: 'call-2' }),
        event(4, 'tool/execution_completed', 420, {
          model_call_id: 'call-1',
          tool_call_id: 'tool-1',
          status: 'completed',
        }),
        event(3, 'tool/execution_started', 120, {
          model_call_id: 'call-1',
          tool_call_id: 'tool-1',
          tool_name: 'Read',
        }),
        event(2, 'llm/response', 90, { model_call_id: 'call-1' }),
        event(1, 'llm/request', 10, { model_call_id: 'call-1' }),
      ],
      [call('call-1', [tool('tool-1', 'Read')]), call('call-2')],
    );

    expect(rows.map((row) => row.eventSeq)).toEqual([1, 2, 3, 5, 6]);
    expect(rows.map((row) => row.eventType)).toEqual([
      'llm/request',
      'llm/response',
      'tool/execution',
      'llm/request',
      'llm/response',
    ]);
    expect(rows[2]?.eventSeqs).toEqual([3, 4]);
    expect(rows[2]?.target).toEqual({
      modelCallId: 'call-1',
      stage: 'tool',
      toolCallId: 'tool-1',
    });
  });

  test('uses terminal duration first and lifecycle delta as the fallback', () => {
    const rows = buildTimelineRows(
      [
        event(2, 'tool/execution_completed', 450, {
          model_call_id: 'call-1',
          tool_call_id: 'tool-1',
        }),
        event(1, 'tool/execution_started', 200, {
          model_call_id: 'call-1',
          tool_call_id: 'tool-1',
        }),
        event(4, 'tool/execution_completed', 900, {
          model_call_id: 'call-1',
          tool_call_id: 'tool-2',
          duration_ms: 33,
        }),
        event(3, 'tool/execution_started', 800, {
          model_call_id: 'call-1',
          tool_call_id: 'tool-2',
        }),
      ],
      [call('call-1', [tool('tool-1', 'Read'), tool('tool-2', 'Write')])],
    );

    expect(rows[0]?.durationMs).toBe(250);
    expect(rows[1]?.durationMs).toBe(33);
  });

  test('keeps unmatched lifecycle events and auxiliary calls selectable or labelled', () => {
    const rows = buildTimelineRows(
      [
        event(3, 'observation/gap', 300),
        event(2, 'tool/execution_started', 200, {
          model_call_id: 'call-1',
          tool_call_id: 'unmatched',
          call_kind: 'session_auxiliary',
        }),
        event(1, 'turn/start', 0),
      ],
      [call('call-1')],
    );

    expect(rows.map((row) => row.eventType)).toEqual([
      'turn/start',
      'tool/execution_started',
      'observation/gap',
    ]);
    expect(rows[1]?.callKind).toBe('session_auxiliary');
    expect(rows[0]?.target).toBeNull();
    expect(findTimelineEventSeq(rows, null)).toBeNull();
  });

  test('shows only waits at or above one second and maps grouped tool rows', () => {
    const rows = buildTimelineRows(
      [
        event(4, 'turn/end', 1999),
        event(3, 'llm/response', 999, { model_call_id: 'call-1' }),
        event(2, 'llm/request', 100, { model_call_id: 'call-1' }),
        event(1, 'turn/start', 0),
      ],
      [call('call-1')],
    );

    expect(timelineWaitDurations(rows)).toEqual([1000]);
    expect(
      findTimelineEventSeq(rows, { modelCallId: 'call-1', stage: 'response' }),
    ).toBe(3);
  });

  test('keeps only relative time as the compact rail label', () => {
    const rows = buildTimelineRows(
      [
        event(4, 'tool/execution_completed', 300, {
          model_call_id: 'call-1',
          tool_call_id: 'tool-1',
          status: 'completed',
        }),
        event(3, 'tool/execution_started', 100, {
          model_call_id: 'call-1',
          tool_call_id: 'tool-1',
          tool_name: 'Write-Output',
        }),
        event(2, 'llm/response', 80, { model_call_id: 'call-1' }),
        event(1, 'llm/request', 0, { model_call_id: 'call-1' }),
      ],
      [call('call-1', [tool('tool-1', 'Write-Output')])],
    );

    expect(timelineRowRelativeTime(rows[0]!)).toBe('+0s');
    expect(timelineRowRelativeTime(rows[1]!)).toBe('+0.1s');
    expect(timelineRowRelativeTime(rows[2]!)).toBe('+0.1s');
  });

  test('maps every selectable timeline event to a semantic icon kind', () => {
    const rows = buildTimelineRows(
      [
        event(9, 'turn/end', 900),
        event(8, 'observation/gap', 800),
        event(7, 'llm/response', 700, { model_call_id: 'call-2' }),
        event(6, 'llm/request', 600, { model_call_id: 'call-2' }),
        event(5, 'tool/execution_completed', 500, {
          model_call_id: 'call-1',
          tool_call_id: 'tool-1',
          status: 'completed',
        }),
        event(4, 'tool/execution_started', 400, {
          model_call_id: 'call-1',
          tool_call_id: 'tool-1',
        }),
        event(3, 'llm/response', 300, { model_call_id: 'call-1' }),
        event(2, 'llm/request', 200, { model_call_id: 'call-1' }),
        event(1, 'turn/start', 0),
      ],
      [call('call-1', [tool('tool-1', 'Read')]), call('call-2')],
    );

    expect(rows.map((row) => timelineRowIconKind(row))).toEqual([
      'start',
      'request',
      'response-tool',
      'tool',
      'request',
      'response-final',
      'gap',
      'end',
    ]);
  });

  test('gives every selectable row a compact relative time', () => {
    const rows = buildTimelineRows(
      [
        event(7, 'turn/end', 700),
        event(6, 'observation/gap', 600),
        event(5, 'tool/execution_started', 500, {
          model_call_id: 'call-1',
          tool_call_id: 'tool-1',
        }),
        event(4, 'llm/response', 400, { model_call_id: 'call-1' }),
        event(3, 'llm/request', 300, { model_call_id: 'call-1' }),
        event(2, 'session_workflow', 200),
        event(1, 'turn/start', 0),
      ],
      [call('call-1', [tool('tool-1', 'Read')])],
    );

    for (const row of rows) {
      expect(timelineRowRelativeTime(row)).toMatch(/^\+/);
    }
  });
});
