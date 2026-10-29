import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const componentSource = readFileSync(new URL('./SessionSkillModeBar.tsx', import.meta.url), 'utf8');
const nomiChatSource = readFileSync(
  new URL('../platforms/nomi/NomiChat.tsx', import.meta.url),
  'utf8'
);
const acpChatSource = readFileSync(
  new URL('../platforms/acp/AcpChat.tsx', import.meta.url),
  'utf8'
);

describe('Session skill mode bar', () => {
  test('renders auto-load chips with a pin control', () => {
    expect(componentSource.includes("data-testid='session-skill-mode-bar'")).toBe(true);
    expect(componentSource.includes('listSkillPins')).toBe(true);
    expect(componentSource.includes('setSkillPin')).toBe(true);
    expect(componentSource.includes("invocation === 'model-auto'")).toBe(true);
  });

  test('is mounted on Nomi and ACP session surfaces', () => {
    expect(nomiChatSource.includes('<SessionSkillModeBar')).toBe(true);
    expect(acpChatSource.includes('<SessionSkillModeBar')).toBe(true);
  });
});
