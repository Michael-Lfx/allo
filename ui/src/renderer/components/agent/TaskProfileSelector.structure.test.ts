import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./TaskProfileSelector.tsx', import.meta.url), 'utf8');
const styles = readFileSync(new URL('./TaskProfileSelector.module.css', import.meta.url), 'utf8');

describe('TaskProfileSelector segmented control', () => {
  test('opens the two work modes from a single dropdown chip', () => {
    expect(source.includes('<Dropdown')).toBe(true);
    expect(source.includes("data-testid='task-profile-selector'")).toBe(true);
    expect(source.includes("data-button-shape='pill'")).toBe(true);
    expect(source.includes("data-testid='task-profile-dropdown-menu'")).toBe(true);
    expect(source.includes('task-profile-option-${value}')).toBe(true);
    expect(source.includes("'office'")).toBe(true);
    expect(source.includes("'coding'")).toBe(true);
    expect(source.includes("role='radiogroup'")).toBe(false);
    expect(source.includes('<GooeyNav')).toBe(false);
    expect(source.includes('Briefcase')).toBe(true);
    expect(source.includes('Code')).toBe(true);
  });

  test('does not hardcode a black or white selected pill in the local stylesheet', () => {
    expect(styles.includes('#000')).toBe(false);
    expect(styles.includes('#fff')).toBe(false);
    expect(styles.includes('.optionIcon')).toBe(true);
  });
});
