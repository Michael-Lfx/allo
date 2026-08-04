import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const componentSource = readFileSync(new URL('./MessageSkillLoad.tsx', import.meta.url), 'utf8');
const messageListSource = readFileSync(new URL('../MessageList.tsx', import.meta.url), 'utf8');

describe('Skill load history renderer', () => {
  test('renders the immutable snapshot as a collapsed detail entry', () => {
    expect(componentSource.includes("data-testid='message-skill-load'")).toBe(true);
    expect(componentSource.includes('<details')).toBe(true);
    expect(componentSource.includes('{content}')).toBe(true);
    expect(componentSource.includes('version_hash')).toBe(true);
  });

  test('keeps Skill-only history visible without a blank user transport row', () => {
    expect(messageListSource.includes("case 'skill_load':")).toBe(true);
    expect(messageListSource.includes('<MessageSkillLoad message={message}></MessageSkillLoad>')).toBe(true);
    expect(messageListSource.includes("message.type === 'text' && message.position === 'right' && message.content.content.trim().length === 0")).toBe(true);
  });
});
