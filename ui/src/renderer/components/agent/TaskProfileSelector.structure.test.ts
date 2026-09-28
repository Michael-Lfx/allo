import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./TaskProfileSelector.tsx', import.meta.url), 'utf8');
const styles = readFileSync(new URL('./TaskProfileSelector.module.css', import.meta.url), 'utf8');

describe('TaskProfileSelector segmented control', () => {
  test('renders a radiogroup of work-mode pills instead of a dropdown', () => {
    expect(source.includes("role='radiogroup'")).toBe(true);
    expect(source.includes("role='radio'")).toBe(true);
    expect(source.includes("data-testid='task-profile-selector'")).toBe(true);
    expect(source.includes("data-testid={`task-profile-option-${value}`}")).toBe(true);
    expect(source.includes("'office'")).toBe(true);
    expect(source.includes("'coding'")).toBe(true);
    expect(source.includes('<Dropdown')).toBe(false);
    expect(source.includes('task-profile-dropdown-menu')).toBe(false);
    expect(source.includes('Briefcase')).toBe(true);
    expect(source.includes('Code')).toBe(true);
  });

  test('uses inverted selected styling through theme tokens', () => {
    expect(styles.includes('background: var(--color-text-1)')).toBe(true);
    expect(styles.includes('color: var(--color-bg-1)')).toBe(true);
    expect(styles.includes('border-radius: 999px')).toBe(true);
    expect(styles.includes('#000')).toBe(false);
    expect(styles.includes('#fff')).toBe(false);
  });
});
