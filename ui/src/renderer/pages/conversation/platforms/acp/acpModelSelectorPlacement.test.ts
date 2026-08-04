

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
});
