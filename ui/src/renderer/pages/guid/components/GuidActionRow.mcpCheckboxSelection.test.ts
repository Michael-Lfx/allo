/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const actionRowSource = readFileSync(new URL('./GuidActionRow.tsx', import.meta.url), 'utf8');
const modelSelectorSource = readFileSync(new URL('./GuidModelSelector.tsx', import.meta.url), 'utf8');
const guidCss = readFileSync(new URL('../index.module.css', import.meta.url), 'utf8');
const controlCss = readFileSync(new URL('../../../styles/theme-control-contract.css', import.meta.url), 'utf8');

describe('GuidActionRow MCP checkbox selection treatment', () => {
  test('applies the enhanced theme-aware checkbox treatment to MCP server choices', () => {
    expect(actionRowSource.includes("className='guid-mcp-selection-checkbox'")).toBe(true);
    expect(controlCss.includes('.arco-checkbox-checked .arco-checkbox-mask')).toBe(true);
    expect(controlCss.includes('.arco-checkbox-mask-icon')).toBe(true);
  });

  test('collapses model configuration labels to hover-expand icons in a narrow action slot', () => {
    expect(actionRowSource.includes('styles.actionConfigGroupResponsive')).toBe(true);
    expect(actionRowSource.includes('styles.actionSubmitResponsive')).toBe(true);
    expect(actionRowSource.includes('modelSelectorIsChat')).toBe(true);
    expect(actionRowSource.includes('chat-model-picker-slot')).toBe(true);
    expect(actionRowSource.includes('sendbox-strategy-slot')).toBe(true);
    expect(modelSelectorSource.includes('sendbox-responsive-label')).toBe(true);
    expect(modelSelectorSource.includes('sendbox-responsive-chevron')).toBe(true);
    expect(modelSelectorSource.includes('<Tooltip')).toBe(false);
    expect(guidCss.includes('container-name: guid-action-config')).toBe(true);
    expect(guidCss.includes('@container guid-action-config (max-width: 440px)')).toBe(true);
    expect(guidCss.includes(':global(.guid-config-btn:not(.chat-model-picker-trigger):hover)')).toBe(true);
    expect(modelSelectorSource.includes('flowy-icon-text-btn')).toBe(true);
    expect(modelSelectorSource.includes('flowy-button-inline-content')).toBe(true);
    expect(actionRowSource.includes('flowy-icon-text-btn')).toBe(true);
    expect(actionRowSource.includes('flowy-button-inline-content')).toBe(true);
    expect(guidCss.includes('.flowy-button-inline-content')).toBe(true);
    expect(guidCss.includes('@media (hover: hover) and (pointer: fine)')).toBe(true);
    const stableSlotCss = guidCss.slice(guidCss.indexOf('/* Keep the FlowY chat controls'));
    expect(stableSlotCss.includes('height: 28px')).toBe(true);
    expect(stableSlotCss.includes('position: absolute !important')).toBe(true);
});

  test('places the mode selector immediately after the add button', () => {
    const addButtonIndex = actionRowSource.indexOf("data-testid='file-upload-btn'");
    const modeSelectorIndex = actionRowSource.indexOf('<AgentModeSelector');

    expect(addButtonIndex).toBeGreaterThan(-1);
    expect(modeSelectorIndex).toBeGreaterThan(addButtonIndex);
  });

  test('does not nest Tooltip around the MCP dropdown trigger', () => {
    const mcpBlockStart = actionRowSource.indexOf('{mcpServers.length > 0 && (');
    const mcpBlockEnd = actionRowSource.indexOf('{showModeSwitch && (');

    expect(mcpBlockStart).toBeGreaterThan(-1);
    expect(mcpBlockEnd).toBeGreaterThan(mcpBlockStart);

    const mcpBlock = actionRowSource.slice(mcpBlockStart, mcpBlockEnd);
    expect(mcpBlock.includes('<Dropdown')).toBe(true);
    expect(mcpBlock.includes("data-testid='guid-mcp-menu-btn'")).toBe(true);
    expect(mcpBlock.includes('<Tooltip')).toBe(false);
    expect(mcpBlock.includes('<Tool theme')).toBe(true);
    expect(mcpBlock.includes('<Shield')).toBe(false);
  });
});
