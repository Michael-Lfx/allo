/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { projectObservationScan } from './observationScan';

describe('observationScan', () => {
  test('projects a user text message', () => {
    const result = projectObservationScan(
      [{ role: 'user', content: [{ type: 'text', text: '查一下上海天气' }] }],
      'messages'
    );
    expect(result).toEqual({
      kind: 'messages',
      rows: [
        {
          index: 0,
          role: 'user',
          kinds: ['text'],
          preview: { kind: 'text', text: '查一下上海天气' },
        },
      ],
    });
  });

  test('projects an assistant tool_use without text', () => {
    const result = projectObservationScan(
      [
        {
          role: 'assistant',
          content: [{ type: 'tool_use', id: 'c1', name: 'weather', input: { city: '上海' } }],
        },
      ],
      'messages'
    );
    expect(result.kind).toBe('messages');
    if (result.kind !== 'messages') return;
    expect(result.rows[0]?.preview).toEqual({ kind: 'tool_use', name: 'weather' });
    expect(result.rows[0]?.kinds).toEqual(['tool_use']);
  });

  test('flags an error tool_result', () => {
    const result = projectObservationScan(
      [
        {
          role: 'tool',
          content: [
            {
              type: 'tool_result',
              tool_use_id: 'c1',
              content: 'timeout',
              is_error: true,
            },
          ],
        },
      ],
      'messages'
    );
    expect(result.kind).toBe('messages');
    if (result.kind !== 'messages') return;
    expect(result.rows[0]?.preview).toEqual({
      kind: 'tool_result',
      text: 'timeout',
      isError: true,
    });
  });

  test('projects thinking when it is the only block', () => {
    const result = projectObservationScan(
      [{ role: 'assistant', content: [{ type: 'thinking', thinking: '先看城市' }] }],
      'messages'
    );
    expect(result.kind).toBe('messages');
    if (result.kind !== 'messages') return;
    expect(result.rows[0]?.preview).toEqual({ kind: 'thinking', text: '先看城市' });
  });

  test('projects an image as media type without inventing pixels', () => {
    const result = projectObservationScan(
      [
        {
          role: 'user',
          content: [
            {
              type: 'image',
              media_type: 'image/png',
              data: { omitted_reason: 'binary_payload', byte_length: 12 },
            },
          ],
        },
      ],
      'messages'
    );
    expect(result.kind).toBe('messages');
    if (result.kind !== 'messages') return;
    expect(result.rows[0]?.preview).toEqual({ kind: 'image', mediaType: 'image/png' });
    expect(result.rows[0]?.omittedReason).toBe('binary_payload');
  });

  test('prefers the first text block over later tool_use', () => {
    const result = projectObservationScan(
      [
        {
          role: 'assistant',
          content: [
            { type: 'tool_use', id: 'c1', name: 'weather', input: {} },
            { type: 'text', text: '马上查' },
          ],
        },
      ],
      'messages'
    );
    expect(result.kind).toBe('messages');
    if (result.kind !== 'messages') return;
    expect(result.rows[0]?.preview).toEqual({ kind: 'text', text: '马上查' });
    expect(result.rows[0]?.kinds).toEqual(['tool_use', 'text']);
  });

  test('keeps unknown roles as written', () => {
    const result = projectObservationScan(
      [{ role: 'developer', content: [{ type: 'text', text: 'hi' }] }],
      'messages'
    );
    expect(result.kind).toBe('messages');
    if (result.kind !== 'messages') return;
    expect(result.rows[0]?.role).toBe('developer');
  });

  test('marks a whole messages payload omitted instead of an empty list', () => {
    expect(
      projectObservationScan(
        { omitted_reason: 'event_size_limit', original_bytes: 200000, captured_bytes: 0 },
        'messages'
      )
    ).toEqual({ kind: 'omitted', reason: 'event_size_limit' });
  });

  test('projects tool names, full descriptions, and deferred', () => {
    const result = projectObservationScan(
      [
        {
          name: 'weather',
          description: 'Get weather\nMore detail that must not show',
          input_schema: { omitted_reason: 'input_schema_elided' },
        },
        {
          name: 'read_file',
          description: 'Read a file',
          deferred: true,
          input_schema: { omitted_reason: 'input_schema_elided' },
        },
      ],
      'tools'
    );
    expect(result).toEqual({
      kind: 'tools',
      rows: [
        { index: 0, name: 'weather', description: 'Get weather\nMore detail that must not show', deferred: false },
        { index: 1, name: 'read_file', description: 'Read a file', deferred: true },
      ],
    });
  });

  test('marks a whole tools payload omitted', () => {
    expect(
      projectObservationScan({ omitted_reason: 'event_size_limit' }, 'tools')
    ).toEqual({ kind: 'omitted', reason: 'event_size_limit' });
  });

  test('does not treat null or a string as a scan list', () => {
    expect(projectObservationScan(null, 'messages')).toEqual({ kind: 'unscannable' });
    expect(projectObservationScan('hello', 'messages')).toEqual({ kind: 'unscannable' });
  });

  test('keeps full message text for hover', () => {
    const long = '测'.repeat(90);
    const result = projectObservationScan(
      [{ role: 'user', content: [{ type: 'text', text: long }] }],
      'messages'
    );
    expect(result.kind).toBe('messages');
    if (result.kind !== 'messages') return;
    expect(result.rows[0]?.preview).toEqual({ kind: 'text', text: long });
  });
});
