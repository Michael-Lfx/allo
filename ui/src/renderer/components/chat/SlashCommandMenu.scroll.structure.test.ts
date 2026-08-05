import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('slash command menu keyboard scrolling', () => {
  test('does not delegate active-item scrolling to page-level scrollIntoView', () => {
    const source = readSource(new URL('./SlashCommandMenu.tsx', import.meta.url));

    // The launcher is rendered in an absolutely positioned overlay. Native
    // scrollIntoView can scroll the conversation/page when the overlay is at
    // the viewport edge; active-item scrolling must stay within its listbox.
    expect(source.includes('scrollIntoView(')).toBe(false);
    expect(source.includes('overflow-y-auto')).toBe(true);
    expect(source.includes('ref={listRef}')).toBe(true);
    expect(source.includes('list.scrollTop +=')).toBe(true);
  });
});
