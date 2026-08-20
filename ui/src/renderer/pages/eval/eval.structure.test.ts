import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('agent eval lab', () => {
  test('gates the page and sider entry on developer mode', () => {
    const page = readSource(new URL('./index.tsx', import.meta.url));
    const sider = readSource(new URL('../../components/layout/Sider/index.tsx', import.meta.url));
    const entry = readSource(
      new URL('../../components/layout/Sider/SiderNav/SiderEvalEntry.tsx', import.meta.url)
    );
    const router = readSource(new URL('../../components/layout/Router.tsx', import.meta.url));

    expect(page.includes("useConfig('system.developerMode')")).toBe(true);
    expect(page.includes("Navigate to='/guid'")).toBe(true);
    expect(page.includes('/api/debug/agent-evals')).toBe(false);
    expect(page.includes("useState('office_tasks')")).toBe(true);
    expect(page.includes('evalApi.startRun')).toBe(true);
    expect(page.includes('evalApi.cancelRun')).toBe(true);
    expect(page.includes('evalApi.getCaseTrace')).toBe(true);
    expect(page.includes('evalApi.getCaseObservation')).toBe(true);
    expect(page.includes('current_trace')).toBe(true);
    expect(page.includes('conversation_id')).toBe(true);
    expect(page.includes('workspace_label')).toBe(true);
    expect(page.includes('TraceView')).toBe(true);
    expect(sider.includes('SiderEvalEntry')).toBe(true);
    expect(sider.includes("useConfig('system.developerMode')")).toBe(true);
    expect(sider.includes('developerMode === true')).toBe(true);
    expect(entry.includes("t('eval.dev.tag')")).toBe(true);
    expect(router.includes("path='/eval'")).toBe(true);
  });

  test('talks to the live eval debug API', () => {
    const api = readSource(new URL('./api.ts', import.meta.url));
    expect(api.includes("const BASE = '/api/debug/agent-evals'")).toBe(true);
    expect(api.includes('${BASE}/suites')).toBe(true);
    expect(api.includes('${BASE}/runs')).toBe(true);
    expect(api.includes('/cancel')).toBe(true);
    expect(api.includes('/pull')).toBe(true);
    expect(api.includes('/cases/')).toBe(true);
    expect(api.includes('/trace')).toBe(true);
    expect(api.includes('/observation')).toBe(true);
    expect(api.includes('getCaseTrace')).toBe(true);
    expect(api.includes('getCaseObservation')).toBe(true);
  });
});
