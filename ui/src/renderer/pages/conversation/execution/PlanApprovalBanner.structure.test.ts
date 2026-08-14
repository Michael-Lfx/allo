import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./PlanApprovalBanner.tsx', import.meta.url), 'utf8');

describe('PlanApprovalBanner Approval Card adapter', () => {
  test('renders the plan-gate prompt as an Approval Card without changing approve IPC', () => {
    expect(source.includes('<ApprovalCard')).toBe(true);
    expect(source.includes("kind='plan'")).toBe(true);
    expect(source.includes("id: 'approve'")).toBe(true);
    expect(source.includes("label: t('agentExecution.approval.button'")).toBe(true);
    expect(source.includes('ipcBridge.agentExecution.approve.invoke')).toBe(true);
    const optionBlock = source.slice(
      source.indexOf('options='),
      source.indexOf('selectedId='),
    );
    expect(optionBlock.includes("agentExecution.approval.ok")).toBe(false);
    expect(source.includes('refreshOnVersionConflict')).toBe(true);
    expect(source.includes('flex-shrink-0')).toBe(true);
  });
});
