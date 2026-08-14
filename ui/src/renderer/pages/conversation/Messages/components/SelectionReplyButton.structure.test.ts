import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./SelectionReplyButton.tsx', import.meta.url), 'utf8');

describe('SelectionReplyButton', () => {
  test('restyles onto SelectionActions and keeps Shadow DOM quote/reply', () => {
    expect(source.includes('function getEffectiveSelection')).toBe(true);
    expect(source.includes('shadowRoot')).toBe(true);
    expect(source.includes("from '@renderer/components/beautifulUi/selectionActions/SelectionActions'")).toBe(
      true
    );
    expect(source.includes('<SelectionActions')).toBe(true);
    expect(source.includes("id: 'quote'")).toBe(true);
    expect(source.includes("emitter.emit('sendbox.reply'")).toBe(true);
    expect(source.includes('ipcBridge')).toBe(false);
    expect(source.includes("id: 'explain'")).toBe(false);
    expect(source.includes("id: 'improve'")).toBe(false);
    expect(source.includes("id: 'shorten'")).toBe(false);
    expect(source.includes("id: 'tone'")).toBe(false);
    expect(source.includes("id: 'grammar'")).toBe(false);
  });
});
