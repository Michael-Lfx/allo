import { describe, expect, test } from 'bun:test';
import { displayNameForCodeLanguage, filenameFromFenceNode } from './codeBlockLanguage';

describe('displayNameForCodeLanguage', () => {
  test('maps fence aliases to Beautiful UI language labels', () => {
    expect(displayNameForCodeLanguage('rust')).toBe('Rust');
    expect(displayNameForCodeLanguage('ts')).toBe('TypeScript');
    expect(displayNameForCodeLanguage('typescript')).toBe('TypeScript');
    expect(displayNameForCodeLanguage('js')).toBe('JavaScript');
  });

  test('capitalizes unknown languages instead of inventing a filename', () => {
    expect(displayNameForCodeLanguage('zig')).toBe('Zig');
    expect(displayNameForCodeLanguage('')).toBe('');
    expect(displayNameForCodeLanguage(undefined)).toBe('');
  });
});

describe('filenameFromFenceNode', () => {
  test('reads a dotted token or filename= from fence meta', () => {
    expect(filenameFromFenceNode({ data: { meta: 'churn.ts' } })).toBe('churn.ts');
    expect(filenameFromFenceNode({ properties: { meta: 'filename="lib.rs"' } })).toBe('lib.rs');
    expect(filenameFromFenceNode({ data: { meta: '{1,3}' } })).toBeUndefined();
    expect(filenameFromFenceNode(null)).toBeUndefined();
  });
});
