import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./FileChangesPanel.tsx', import.meta.url), 'utf8');

describe('FileChangesPanel', () => {
  test('is a thin adapter over DiffTable and still exports FileChangeItem', () => {
    expect(source.includes('export interface FileChangeItem')).toBe(true);
    expect(source.includes("from '@renderer/components/beautifulUi/diffTable/DiffTable'")).toBe(true);
    expect(source.includes('<DiffTable')).toBe(true);
    expect(source.includes('onFileClick')).toBe(true);
    expect(source.includes('onDiffClick')).toBe(true);
    expect(source.includes('PreviewOpen')).toBe(false);
    expect(source.includes('TMessageType')).toBe(false);
  });
});
