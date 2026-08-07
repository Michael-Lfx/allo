import { describe, expect, test } from 'bun:test';
import { handleComposerPasteEvent } from './ComposerSkillTokenInput';

type ComposerPasteEvent = Parameters<typeof handleComposerPasteEvent>[0];

function createPasteEvent(options: { text?: string; files?: unknown[]; defaultPrevented?: boolean }) {
  const counters = {
    preventDefault: 0,
    stopPropagation: 0,
    order: [] as string[],
  };
  const event = {
    clipboardData: {
      files: (options.files ?? []) as unknown as FileList,
      getData: () => options.text ?? '',
    },
    defaultPrevented: options.defaultPrevented ?? false,
    preventDefault() {
      counters.order.push('preventDefault');
      counters.preventDefault += 1;
      event.defaultPrevented = true;
    },
    stopPropagation() {
      counters.order.push('stopPropagation');
      counters.stopPropagation += 1;
    },
  } as unknown as ComposerPasteEvent;

  return { event, counters };
}

describe('ComposerSkillTokenInput paste ownership', () => {
  test('ignores an event that was already handled', () => {
    const { event, counters } = createPasteEvent({ text: 'ABC', defaultPrevented: true });
    const inserted: string[] = [];
    let delegated = 0;

    handleComposerPasteEvent(event, (text) => inserted.push(text), () => delegated++);

    expect(inserted).toEqual([]);
    expect(delegated).toBe(0);
    expect(counters.preventDefault).toBe(0);
    expect(counters.stopPropagation).toBe(0);
  });

  test('handles plain text once and stops the event before writing', () => {
    const { event, counters } = createPasteEvent({ text: 'ABC\n  ' });
    const inserted: string[] = [];
    let delegated = 0;

    handleComposerPasteEvent(
      event,
      (text) => {
        counters.order.push('replaceSelectionWithText');
        inserted.push(text);
      },
      () => delegated++
    );

    expect(inserted).toEqual(['ABC']);
    expect(delegated).toBe(0);
    expect(counters.preventDefault).toBe(1);
    expect(counters.stopPropagation).toBe(1);
    expect(counters.order).toEqual(['preventDefault', 'stopPropagation', 'replaceSelectionWithText']);
  });

  test('delegates file paste without inserting file text', () => {
    const { event, counters } = createPasteEvent({ text: 'image.png', files: [{}] });
    const inserted: string[] = [];
    let delegated = 0;

    handleComposerPasteEvent(event, (text) => inserted.push(text), () => delegated++);

    expect(inserted).toEqual([]);
    expect(delegated).toBe(1);
    expect(counters.preventDefault).toBe(0);
    expect(counters.stopPropagation).toBe(0);
  });

  test('does not deduplicate two independent identical paste events', () => {
    const inserted: string[] = [];

    for (let index = 0; index < 2; index += 1) {
      const { event } = createPasteEvent({ text: 'ABC' });
      handleComposerPasteEvent(event, (text) => inserted.push(text));
    }

    expect(inserted).toEqual(['ABC', 'ABC']);
  });
});
