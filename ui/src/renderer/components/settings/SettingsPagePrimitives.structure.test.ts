import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./SettingsPagePrimitives.tsx', import.meta.url), 'utf8');

describe('settings page spacing contract', () => {
  test('separates nested rows and footer actions without visible rules', () => {
    expect(source).toContain('mb-12px ml-12px pl-12px md:ml-20px md:pl-20px');
    expect(source).toContain("classNames('flex pt-16px', className)");
    expect(source).not.toContain('border-t');
    expect(source).not.toContain('border-l');
  });
});
