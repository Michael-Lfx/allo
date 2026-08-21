/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { joinOmittedMark, omittedReasonOf, projectObservationScan } from './observationScan';

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

  test('keeps the real user text primary when context comes first', () => {
    const result = projectObservationScan(
      [
        {
          role: 'user',
          content: [
            { type: 'text', text: '[Context]\nCurrent date: 2026-08-21' },
            { type: 'text', text: '66' },
          ],
        },
      ],
      'messages'
    );
    expect(result).toEqual({
      kind: 'messages',
      rows: [
        {
          index: 0,
          role: 'user',
          kinds: ['text'],
          preview: { kind: 'text', text: '66' },
          context: { text: 'Current date: 2026-08-21' },
        },
      ],
    });
  });

  test('applies the same Context rule to scalar user content', () => {
    const result = projectObservationScan(
      [
        { role: 'user', content: '[Context]\nCurrent date: 2026-08-21' },
        { role: 'assistant', content: '[Context]\nliteral assistant text' },
      ],
      'messages',
    );
    expect(result).toEqual({
      kind: 'messages',
      rows: [
        {
          index: 0,
          role: 'user',
          kinds: [],
          preview: { kind: 'empty' },
          context: { text: 'Current date: 2026-08-21' },
        },
        {
          index: 1,
          role: 'assistant',
          kinds: ['text'],
          preview: { kind: 'text', text: '[Context]\nliteral assistant text' },
        },
      ],
    });
  });

  test('keeps context metadata separate across multiple turns', () => {
    const result = projectObservationScan(
      [
        { role: 'user', content: [{ type: 'text', text: '66' }] },
        { role: 'assistant', content: [{ type: 'text', text: '收到' }] },
        {
          role: 'user',
          content: [
            { type: 'text', text: '[Context]\nCurrent date: 2026-08-21' },
            { type: 'text', text: '11' },
          ],
        },
      ],
      'messages'
    );
    expect(result.kind).toBe('messages');
    if (result.kind !== 'messages') return;
    expect(result.rows.map((row) => row.preview)).toEqual([
      { kind: 'text', text: '66' },
      { kind: 'text', text: '收到' },
      { kind: 'text', text: '11' },
    ]);
    expect(result.rows[2]?.context).toEqual({ text: 'Current date: 2026-08-21' });
  });

  test('keeps a context-only message as an empty primary row', () => {
    const result = projectObservationScan(
      [
        {
          role: 'user',
          content: [{ type: 'text', text: '[Context]\nCurrent date: 2026-08-21' }],
        },
      ],
      'messages'
    );
    expect(result.kind).toBe('messages');
    if (result.kind !== 'messages') return;
    expect(result.rows[0]?.kinds).toEqual([]);
    expect(result.rows[0]?.preview).toEqual({ kind: 'empty' });
    expect(result.rows[0]?.context).toEqual({ text: 'Current date: 2026-08-21' });
  });

  test('only treats a leading user context block as injected context', () => {
    const result = projectObservationScan(
      [
        {
          role: 'assistant',
          content: [
            { type: 'text', text: '[Context]\nassistant literal' },
            { type: 'text', text: 'assistant reply' },
          ],
        },
        {
          role: 'user',
          content: [
            { type: 'text', text: 'user text' },
            { type: 'text', text: '[Context]\nuser literal' },
          ],
        },
      ],
      'messages'
    );
    expect(result.kind).toBe('messages');
    if (result.kind !== 'messages') return;
    expect(result.rows.map((row) => row.preview)).toEqual([
      { kind: 'text', text: '[Context]\nassistant literal' },
      { kind: 'text', text: 'user text' },
    ]);
    expect(result.rows.every((row) => row.context === undefined)).toBe(true);
  });

  test('keeps an unmarked omitted leading block out of the Context projection', () => {
    const result = projectObservationScan(
      [
        {
          role: 'user',
          content: [
            { type: 'text', text: { omitted_reason: 'event_size_limit' } },
            { type: 'text', text: '66' },
          ],
        },
      ],
      'messages',
    );
    expect(result.kind).toBe('messages');
    if (result.kind !== 'messages') return;
    expect(result.rows[0]?.context).toBeUndefined();
    expect(result.rows[0]?.preview).toEqual({ kind: 'text', text: '66' });
    expect(result.rows[0]?.omittedReason).toBe('event_size_limit');
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

  test('treats an empty array as a scan list instead of omitted', () => {
    expect(projectObservationScan([], 'messages')).toEqual({ kind: 'messages', rows: [] });
    expect(projectObservationScan([], 'tools')).toEqual({ kind: 'tools', rows: [] });
  });

  test('adds image kind for tool_result screenshots without inventing pixels', () => {
    const result = projectObservationScan(
      [
        {
          role: 'tool',
          content: [
            {
              type: 'tool_result',
              tool_use_id: 'c1',
              content: 'ok',
              is_error: false,
              images: [{ media_type: 'image/png', data: { omitted_reason: 'binary_payload' } }],
            },
          ],
        },
      ],
      'messages'
    );
    expect(result.kind).toBe('messages');
    if (result.kind !== 'messages') return;
    expect(result.rows[0]?.kinds).toEqual(['tool_result', 'image']);
    expect(result.rows[0]?.preview).toEqual({ kind: 'tool_result', text: 'ok', isError: false });
    expect(result.rows[0]?.omittedReason).toBe('binary_payload');
  });

  test('joins omitted marks onto existing previews', () => {
    expect(joinOmittedMark('图片 image/png', 'binary_payload', '已省略')).toBe(
      '图片 image/png · 已省略 · binary_payload'
    );
    expect(joinOmittedMark('', 'event_size_limit', '已省略')).toBe('已省略 · event_size_limit');
  });

  test('marks a whole messages payload omitted instead of an empty list', () => {
    expect(
      projectObservationScan(
        { omitted_reason: 'event_size_limit', original_bytes: 200000, captured_bytes: 0 },
        'messages'
      )
    ).toEqual({ kind: 'omitted', reason: 'event_size_limit' });
  });

  test('keeps omitted response fields distinguishable from missing values', () => {
    expect(omittedReasonOf({ omitted_reason: 'event_size_limit' })).toBe('event_size_limit');
    expect(omittedReasonOf({ omitted_reason: 42 })).toBeUndefined();
    expect(omittedReasonOf(null)).toBeUndefined();
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

  test('marks an omitted tool description instead of an empty string', () => {
    const result = projectObservationScan(
      [{ name: 'weather', description: { omitted_reason: 'event_size_limit' } }],
      'tools'
    );
    expect(result.kind).toBe('tools');
    if (result.kind !== 'tools') return;
    expect(result.rows[0]).toEqual({
      index: 0,
      name: 'weather',
      description: '',
      deferred: false,
      omittedReason: 'event_size_limit',
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
