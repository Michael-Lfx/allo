import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./TaskProfileSelector.tsx', import.meta.url), 'utf8');
const styles = readFileSync(new URL('./TaskProfileSelector.module.css', import.meta.url), 'utf8');

describe('TaskProfileSelector segmented control', () => {
  test('renders a radiogroup of work-mode pills instead of a dropdown', () => {
    expect(source.includes('<GooeyNav')).toBe(true);
    expect(source.includes("selection='radio'")).toBe(true);
    expect(source.includes("testId='task-profile-selector'")).toBe(true);
    expect(source.includes('task-profile-option-${value}')).toBe(true);
    expect(source.includes("'office'")).toBe(true);
    expect(source.includes("'coding'")).toBe(true);
    expect(source.includes('<Dropdown')).toBe(false);
    expect(source.includes('task-profile-dropdown-menu')).toBe(false);
    expect(source.includes('Briefcase')).toBe(true);
    expect(source.includes('Code')).toBe(true);
  });

  test('does not hardcode a black or white selected pill in the local stylesheet', () => {
    expect(styles.includes('#000')).toBe(false);
    expect(styles.includes('#fff')).toBe(false);
    expect(styles.includes('.optionIcon')).toBe(true);
  });
});
