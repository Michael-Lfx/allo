

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('ACP conversation model selector placement', () => {
  test('renders the mode selector after the add button in the sendbox tools', () => {
    const source = readSource(new URL('./AcpSendBox.tsx', import.meta.url));
    const toolsIndex = source.indexOf('tools={');
    const fileAttachIndex = source.indexOf('<FileAttachButton', toolsIndex);
    const modeIndex = source.indexOf('<AgentModeSelector', toolsIndex);
    const rightToolsIndex = source.indexOf('rightTools={');
    const modelIndex = source.indexOf('<AcpModelSelector', rightToolsIndex);

    expect(toolsIndex).toBeGreaterThan(-1);
    expect(fileAttachIndex).toBeGreaterThan(toolsIndex);
    expect(modeIndex).toBeGreaterThan(fileAttachIndex);
    expect(rightToolsIndex).toBeGreaterThan(-1);
    expect(modelIndex).toBeGreaterThan(rightToolsIndex);
  });

  test('does not render ACP model switching in the conversation header', () => {
    const source = readSource(new URL('../../components/ChatConversation.tsx', import.meta.url));

    expect(source.includes('<AcpModelSelector')).toBe(false);
  });

  test('passes the conversation busy state to both ACP model entry points', () => {
    const sendBoxSource = readSource(new URL('./AcpSendBox.tsx', import.meta.url));
    const selectorSource = readSource(new URL('../../../../components/agent/AcpModelSelector.tsx', import.meta.url));
    const hookSource = readSource(new URL('../../../../hooks/agent/useAcpModelInfo.ts', import.meta.url));

    expect(sendBoxSource.includes('isConversationModelSelectionDisabled')).toBe(true);
    expect(sendBoxSource.includes('isBusy,')).toBe(true);
    expect(sendBoxSource.includes('disabled={modelSelectionDisabled}')).toBe(true);
    expect(sendBoxSource.includes('setIsMobileSheetOpen(false)')).toBe(true);
    expect(selectorSource.includes('disabled?: boolean')).toBe(true);
    expect(selectorSource.includes('disabled={disabled}')).toBe(true);
    expect(hookSource.includes('if (!enabled || disabled) return;')).toBe(true);
    expect(hookSource.includes('disabledRef.current')).toBe(true);
    expect(hookSource.includes('enabled && !disabled')).toBe(true);
  });
});
