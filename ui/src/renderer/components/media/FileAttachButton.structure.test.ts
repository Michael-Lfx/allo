import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = () => readFileSync(new URL('./FileAttachButton.tsx', import.meta.url), 'utf8');

describe('desktop add menu', () => {
  test('delegates the desktop add panel to the shared SendBox surface', () => {
    const source = readSource();

    expect(source.includes('if (isDesktop)')).toBe(true);
    expect(source.includes("emitter.emit('sendbox.open-add')")).toBe(true);
    expect(source.includes("t('common.add')")).toBe(false);
    expect(source.includes("t('conversation.goal.menu.description'")).toBe(false);
  });
});
