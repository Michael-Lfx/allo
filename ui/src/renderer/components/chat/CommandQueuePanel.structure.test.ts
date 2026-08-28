import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('CommandQueuePanel Cursor-like queue card', () => {
  test('renders a detached card with count, send now, edit, and delete', () => {
    const source = readSource(new URL('./CommandQueuePanel.tsx', import.meta.url));
    const css = readSource(new URL('./commandQueuePanel.module.css', import.meta.url));

    expect(source.includes("data-testid='command-queue-panel'")).toBe(true);
    expect(source.includes("data-testid='command-queue-count'")).toBe(true);
    expect(source.includes("data-testid='command-queue-send-now'")).toBe(true);
    expect(source.includes('conversation.commandQueue.queuedCount')).toBe(true);
    expect(source.includes('conversation.commandQueue.sendNow')).toBe(true);
    expect(source.includes('onSendNow')).toBe(true);
    expect(source.includes('EditTwo')).toBe(false);
    expect(source.includes("Edit theme='outline'")).toBe(true);
    expect(source.includes('CornerDownLeft')).toBe(true);
    expect(source.includes('CornerDownRight')).toBe(false);
    expect(source.includes('MoreOne')).toBe(false);
    expect(source.includes('mb--12px')).toBe(false);
    expect(source.includes('rd-t-18px')).toBe(false);
    expect(css.includes('border-radius: 16px')).toBe(true);
    expect(css.includes('scale(0.96)')).toBe(true);
    expect(css.includes('scale(0.98)')).toBe(true);
    expect(css.includes('@media (hover: hover)')).toBe(true);
    expect(css.includes('prefers-reduced-motion')).toBe(true);
    expect(css.includes('.item.itemDragging')).toBe(true);
    expect(source.includes('itemDragging')).toBe(true);
  });

  test('conversation sendboxes promote a queued item with Send now', () => {
    const nomi = readSource(new URL('../../pages/conversation/platforms/nomi/NomiSendBox.tsx', import.meta.url));
    const acp = readSource(new URL('../../pages/conversation/platforms/acp/AcpSendBox.tsx', import.meta.url));
    const basic = readSource(new URL('../../pages/conversation/platforms/BasicRuntimeSendBox.tsx', import.meta.url));
    const hook = readSource(
      new URL('../../pages/conversation/platforms/useConversationCommandQueue.ts', import.meta.url)
    );
    const helpers = readSource(
      new URL('../../pages/conversation/platforms/commandQueueItems.ts', import.meta.url)
    );

    expect(helpers.includes('export const promoteQueuedCommand')).toBe(true);
    expect(hook.includes("logCommandQueue(conversationKey, 'send-now'")).toBe(true);
    expect(nomi.includes('onSendNow={sendNow}')).toBe(true);
    expect(acp.includes('onSendNow={sendNow}')).toBe(true);
    expect(basic.includes('onSendNow={sendNow}')).toBe(true);
  });
});
