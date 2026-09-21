import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const sendBoxSource = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');
const guidCardSource = readFileSync(
  new URL('../../../pages/guid/components/GuidInputCard.tsx', import.meta.url),
  'utf8'
);

describe('composer focus shortcut', () => {
  test('SendBox and Guid input listen for composer.focus', () => {
    expect(sendBoxSource.includes("useAddEventListener('composer.focus'")).toBe(true);
    expect(sendBoxSource.includes('tokenInputRef.current?.focus()')).toBe(true);
    expect(guidCardSource.includes("'composer.focus'")).toBe(true);
    expect(guidCardSource.includes("tokenInputRef.current?.focus()")).toBe(true);
  });
});
