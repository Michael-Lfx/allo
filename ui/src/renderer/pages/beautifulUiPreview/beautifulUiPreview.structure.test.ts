import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';
import { BEAUTIFUL_UI_CATALOG } from './catalog';

const pageSource = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');
const routerSource = readFileSync(new URL('../../components/layout/Router.tsx', import.meta.url), 'utf8');

describe('Beautiful UI preview page', () => {
  test('registers the hidden test route', () => {
    expect(routerSource.includes("path='/test/beautiful-ui'")).toBe(true);
    expect(routerSource.includes("import('@renderer/pages/beautifulUiPreview')")).toBe(true);
  });

  test('stays outside ProtectedLayout so it does not bounce to login', () => {
    const beautifulIndex = routerSource.indexOf("path='/test/beautiful-ui'");
    const protectedIndex = routerSource.indexOf('element={<ProtectedLayout');
    expect(beautifulIndex).toBeGreaterThan(-1);
    expect(protectedIndex).toBeGreaterThan(-1);
    expect(beautifulIndex).toBeLessThan(protectedIndex);
  });

  test('lists all 19 Beautiful UI primitives and wires Thinking, Streaming Text, Tool Chips, Task Rows, Approval Card, Recommendation Card, Context Cards, Code Block, Diff Table, Selection Actions, and Loading State', () => {
    expect(BEAUTIFUL_UI_CATALOG).toHaveLength(19);
    expect(BEAUTIFUL_UI_CATALOG.filter((item) => item.status === 'preview').map((item) => item.id)).toEqual([
      'thinking',
      'streaming-text',
      'approval-card',
      'tool-chips',
      'task-rows',
      'recommendation-card',
      'context-cards',
      'diff-table',
      'code-block',
      'selection-actions',
      'loading-state',
    ]);
    expect(
      BEAUTIFUL_UI_CATALOG.filter((item) => item.status === 'pending' || item.status === 'skipped').map(
        (item) => item.id
      )
    ).toEqual([
      'chat',
      'prompt-bar',
      'records-table',
      'filter-table',
      'sidebar-nav',
      'search',
      'insight-cards',
      'fine-tune-card',
    ]);
    expect(pageSource.includes("selected.id === 'loading-state'")).toBe(true);
    expect(pageSource.includes('<LoadingState')).toBe(true);
    expect(pageSource.includes('LOADING_STATE_VARIANTS')).toBe(true);
    expect(pageSource.includes("selected.id === 'thinking'")).toBe(true);
    expect(pageSource.includes("selected.id === 'streaming-text'")).toBe(true);
    expect(pageSource.includes("selected.id === 'approval-card'")).toBe(true);
    expect(pageSource.includes("selected.id === 'tool-chips'")).toBe(true);
    expect(pageSource.includes("selected.id === 'task-rows'")).toBe(true);
    expect(pageSource.includes("selected.id === 'recommendation-card'")).toBe(true);
    expect(pageSource.includes("selected.id === 'context-cards'")).toBe(true);
    expect(pageSource.includes("selected.id === 'diff-table'")).toBe(true);
    expect(pageSource.includes("selected.id === 'code-block'")).toBe(true);
    expect(pageSource.includes("selected.id === 'selection-actions'")).toBe(true);
    expect(pageSource.includes('<ThinkingTrace')).toBe(true);
    expect(pageSource.includes('<StreamingText')).toBe(true);
    expect(pageSource.includes('<ApprovalCard')).toBe(true);
    expect(pageSource.includes('<ToolChips')).toBe(true);
    expect(pageSource.includes('<TaskRows')).toBe(true);
    expect(pageSource.includes('<RecommendationCard')).toBe(true);
    expect(pageSource.includes('<ContextCards')).toBe(true);
    expect(pageSource.includes('<DiffTable')).toBe(true);
    expect(pageSource.includes('<CodeBlock')).toBe(true);
    expect(pageSource.includes('<SelectionActions')).toBe(true);
    expect(pageSource.includes('THINKING_RUN_STATES')).toBe(true);
    expect(pageSource.includes('STREAMING_TEXT_STATUSES')).toBe(true);
    expect(pageSource.includes('APPROVAL_KINDS')).toBe(true);
    expect(pageSource.includes('TOOL_CHIP_STATUSES')).toBe(true);
    expect(pageSource.includes('TOOL_CHIP_CONTENT_MODES')).toBe(true);
    expect(pageSource.includes('TASK_ROW_STATUSES')).toBe(true);
    expect(pageSource.includes('TASK_ROW_LAYOUTS')).toBe(true);
    expect(pageSource.includes('RECOMMENDATION_VARIANTS')).toBe(true);
    expect(pageSource.includes('CONTEXT_CARD_CONTENT_MODES')).toBe(true);
    expect(pageSource.includes('CODE_BLOCK_STATUSES')).toBe(true);
  });

  test('does not import conversation message shells', () => {
    expect(pageSource.includes('MessageThinking')).toBe(false);
    expect(pageSource.includes('pages/conversation')).toBe(false);
  });
});
