/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./GuidModelSelector.tsx', import.meta.url), 'utf8');
const cssSource = readFileSync(new URL('../index.module.css', import.meta.url), 'utf8');

describe('GuidModelSelector popup host', () => {
  test('delegates the chat menu to the shared portaled picker', () => {
    expect(source.includes('ChatModelPickerMenu')).toBe(true);
    expect(source.includes('getPopupContainer={() => document.body}')).toBe(true);
    expect(source.includes('chatModelTrigger')).toBe(true);
    expect(source.includes('findChatModelForMenuKey')).toBe(false);
    expect(source.includes('React.forwardRef')).toBe(true);
    expect(source.includes('...rest')).toBe(true);
    expect(source.includes('popupVisible={modelPickerOpen}')).toBe(true);
    expect(source.includes('onVisibleChange={handleModelPickerVisibleChange}')).toBe(true);
    expect(source.includes('popupVisible?: boolean')).toBe(true);
    expect(source.includes('onPopupVisibleChange')).toBe(true);
    expect(source.includes('sendbox-responsive-control-open')).toBe(true);
    expect(source.includes('data-popup-open={modelPickerOpen ? \'true\' : undefined}')).toBe(true);
    expect(source.includes('chat-model-picker-trigger')).toBe(true);
    expect(source.includes("data-layout-part='leading-icon'")).toBe(true);
    expect(source.includes("data-layout-part='chevron'")).toBe(true);
    expect(source.includes("size='11'")).toBe(true);
    expect(source.includes('onClick={() => setSelectedAcpModel')).toBe(false);
    expect(source.includes('conversation.modelPicker.search')).toBe(false);
  });

  test('reuses the parent picker instead of resolving a second Guid catalog', () => {
    expect(source.includes("useModelsForTask('chat', undefined, { enabled: !modelPicker })")).toBe(true);
    expect(source.includes('const pickerGroups = React.useMemo')).toBe(true);
    expect(source.includes('const catalogGroups = modelPicker ? pickerGroups : chatGroups')).toBe(true);
  });

  test('reveals the compact chat model value toward the left on hover or focus', () => {
    expect(cssSource.includes('.chat-model-picker-trigger:hover')).toBe(true);
    expect(cssSource.includes('.chat-model-picker-slot .chat-model-picker-trigger .flowy-button-inline-content')).toBe(true);
    expect(cssSource.includes('flex-basis: var(--chat-model-picker-slot-width) !important;')).toBe(true);
    expect(cssSource.includes('width: var(--chat-model-picker-slot-width) !important;')).toBe(true);
    expect(cssSource.includes('sendbox-responsive-control-open')).toBe(true);
    expect(cssSource.includes(':global(.chat-model-picker-slot .chat-model-picker-trigger.sendbox-responsive-control-open)')).toBe(true);
    expect(cssSource.includes('overflow: hidden !important;')).toBe(true);
    expect(cssSource.includes('justify-content: center !important;')).toBe(true);
    expect(cssSource.includes('overflow: visible !important;')).toBe(true);
    expect(cssSource.includes('flex: 0 0 14px !important;')).toBe(true);
    expect(cssSource.includes('display: inline-grid !important;')).toBe(true);
    expect(cssSource.includes("data-chat-popup='model'")).toBe(true);
    expect(cssSource.includes('visibility: hidden;')).toBe(true);
    expect(cssSource.includes('pointer-events: none;')).toBe(true);
  });
});
