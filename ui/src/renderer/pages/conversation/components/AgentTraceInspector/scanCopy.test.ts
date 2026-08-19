/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import {
  gapSeqLabel,
  requestTileTitle,
  responseTileCopy,
  shouldCloseWorkspaceOnEscape,
} from './scanCopy';

const t = (key: string) => key.split('.').pop() ?? key;

describe('scanCopy', () => {
  test('response tile does not label thinking-only or omitted as tools-only', () => {
    expect(responseTileCopy(t, { has_text: false, has_thinking: true, tool_use_count: 0 }).title).toBe(
      'partThinking'
    );
    expect(
      responseTileCopy(t, {
        has_text: false,
        has_thinking: false,
        text_omitted: true,
        tool_use_count: 0,
      }).title
    ).toBe('omittedField');
    expect(
      responseTileCopy(t, { has_text: false, has_thinking: false, tool_use_count: 2 }).title
    ).toBe('responseToolsOnly');
    expect(responseTileCopy(t, null).title).toBe('responseStage');
    expect(responseTileCopy(t, { has_text: true, has_thinking: false, tool_use_count: 0 }).title).toBe(
      'partText'
    );
  });

  test('request tile does not treat omitted summaries as not recorded', () => {
    expect(requestTileTitle(t, { has_system: false, message_count: 0, tool_definition_count: 0, model: 'm' })).toBe(
      'm'
    );
    expect(
      requestTileTitle(t, {
        has_system: false,
        message_count: 0,
        tool_definition_count: 0,
        messages_omitted: true,
      })
    ).toBe('omittedField');
    expect(
      requestTileTitle(t, { has_system: false, message_count: 0, tool_definition_count: 0 })
    ).toBe('requestStage');
    expect(requestTileTitle(t, null)).toBe('previewMissing');
  });

  test('gap labels stay in i18n instead of seq= dumps', () => {
    expect(gapSeqLabel(t, { event_seq: 12, from_seq: 10, to_seq: 14 })).toBe('gapSeqRange');
    expect(gapSeqLabel(t, { event_seq: 3 })).toBe('gapSeq');
  });

  test('Escape leaves workspace closed when a dialog overlay is open', () => {
    expect(shouldCloseWorkspaceOnEscape({ key: 'Escape', defaultPrevented: false }, false)).toBe(true);
    expect(shouldCloseWorkspaceOnEscape({ key: 'Escape', defaultPrevented: true }, false)).toBe(false);
    expect(shouldCloseWorkspaceOnEscape({ key: 'Escape', defaultPrevented: false }, true)).toBe(false);
    expect(shouldCloseWorkspaceOnEscape({ key: 'Enter', defaultPrevented: false }, false)).toBe(false);
  });
});
