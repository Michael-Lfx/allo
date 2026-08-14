import { describe, expect, test } from 'bun:test';
import {
  buildThinkingTraceItems,
  inferThinkingTraceVariant,
  resolveThinkingTraceStatus,
} from './thinkingTraceModel';

describe('inferThinkingTraceVariant', () => {
  test('keeps unstructured prose as reasoning', () => {
    expect(inferThinkingTraceVariant('The weekend pistachio rush is not a one-off spike.')).toBe(
      'reasoning'
    );
  });

  test('uses subject hints without adding message types', () => {
    expect(inferThinkingTraceVariant('Analyzing...', 'code')).toBe('coding');
    expect(inferThinkingTraceVariant('Looking through docs', 'search')).toBe('search');
    expect(inferThinkingTraceVariant('Plan the work', 'steps')).toBe('steps');
    expect(inferThinkingTraceVariant('code review notes', 'code review')).toBe('coding');
  });

  test('detects numbered and bullet steps from the thinking blob', () => {
    expect(
      inferThinkingTraceVariant('1. Read the POS export\n2. Score stockout risk\n3. Draft the churn order')
    ).toBe('steps');
    expect(inferThinkingTraceVariant('- Read the brief\n- Compare options\n- Draft the answer')).toBe('steps');
  });

  test('detects search and coding traces from content shape', () => {
    expect(
      inferThinkingTraceVariant('Vendor onboarding SOP.pdf\nCold-chain certification\nSales velocity export.csv\nQ4 pistachio +18%')
    ).toBe('search');
    expect(inferThinkingTraceVariant('Read churn.ts\nsrc/kitchen/churn.ts\nEdit reorder.ts\nsrc/inventory/reorder.ts')).toBe(
      'coding'
    );
    expect(
      inferThinkingTraceVariant('```ts\nexport const churn = () => 1;\n```\nThen wire inventory.')
    ).toBe('coding');
  });
});

describe('buildThinkingTraceItems', () => {
  test('returns no items for empty thinking text', () => {
    expect(buildThinkingTraceItems('', 'thinking')).toEqual([]);
    expect(buildThinkingTraceItems('   \n\n  ', 'thinking')).toEqual([]);
  });

  test('keeps a single blob as one running or done item', () => {
    expect(buildThinkingTraceItems('Just reasoning text', 'thinking')).toEqual([
      { id: '0', title: '', detail: 'Just reasoning text', state: 'running' },
    ]);
    expect(buildThinkingTraceItems('Just reasoning text', 'done')).toEqual([
      { id: '0', title: '', detail: 'Just reasoning text', state: 'done' },
    ]);
  });

  test('splits numbered steps and marks live vs settled item states', () => {
    const text = '1. Read the POS export\n3 files\n2. Score stockout risk\nPistachio well\n3. Draft the order\nChurn first';
    expect(buildThinkingTraceItems(text, 'thinking')).toEqual([
      { id: '0', title: 'Read the POS export', detail: '3 files', state: 'done' },
      { id: '1', title: 'Score stockout risk', detail: 'Pistachio well', state: 'done' },
      { id: '2', title: 'Draft the order', detail: 'Churn first', state: 'running' },
    ]);
    expect(buildThinkingTraceItems(text, 'waiting')).toEqual([
      { id: '0', title: 'Read the POS export', detail: '3 files', state: 'done' },
      { id: '1', title: 'Score stockout risk', detail: 'Pistachio well', state: 'done' },
      { id: '2', title: 'Draft the order', detail: 'Churn first', state: 'pending' },
    ]);
    expect(buildThinkingTraceItems(text, 'done')[2]?.state).toBe('done');
    expect(buildThinkingTraceItems(text, 'failed').every((item) => item.state === 'done')).toBe(true);
    expect(buildThinkingTraceItems(text, 'canceled').every((item) => item.state === 'done')).toBe(true);
  });

  test('drops blank thinking segments so empty lines do not reserve a row', () => {
    expect(buildThinkingTraceItems('Visible thought\n\n\n\n', 'done')).toEqual([
      { id: '0', title: '', detail: 'Visible thought', state: 'done' },
    ]);
  });
});

describe('resolveThinkingTraceStatus', () => {
  test('maps message status and process states onto the thinking shell', () => {
    expect(resolveThinkingTraceStatus({ messageStatus: 'thinking' })).toBe('thinking');
    expect(resolveThinkingTraceStatus({ messageStatus: 'done' })).toBe('done');
    expect(resolveThinkingTraceStatus({ messageStatus: 'thinking', forceDone: true })).toBe('done');
    expect(resolveThinkingTraceStatus({ messageStatus: 'thinking', processState: 'running' })).toBe('thinking');
    expect(resolveThinkingTraceStatus({ messageStatus: 'thinking', processState: 'waiting' })).toBe('waiting');
    expect(resolveThinkingTraceStatus({ messageStatus: 'thinking', processState: 'completed' })).toBe('done');
    expect(resolveThinkingTraceStatus({ messageStatus: 'thinking', processState: 'failed' })).toBe('failed');
    expect(resolveThinkingTraceStatus({ messageStatus: 'thinking', processState: 'canceled' })).toBe('canceled');
  });
});
