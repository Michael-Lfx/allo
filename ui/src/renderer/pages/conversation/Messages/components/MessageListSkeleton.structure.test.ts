import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./MessageListSkeleton.tsx', import.meta.url), 'utf8');
const processSource = readFileSync(new URL('./ProcessTraceItem.tsx', import.meta.url), 'utf8');

describe('MessageListSkeleton', () => {
  test('renders the Beautiful UI drive loading state on the outer scroller', () => {
    expect(source.includes("data-testid='message-list-skeleton'")).toBe(true);
    expect(source.includes('<LoadingState')).toBe(true);
    expect(source.includes("variant='drive'") || source.includes('variant="drive"')).toBe(true);
    expect(source.includes('label={')).toBe(true);
  });

  test('does not restyle process-trail agent_status rows onto Loading State', () => {
    expect(processSource.includes("case 'agent_status':")).toBe(true);
    expect(processSource.includes('<ProcessTraceRows')).toBe(true);
    expect(processSource.includes('LoadingState')).toBe(false);
  });
});
