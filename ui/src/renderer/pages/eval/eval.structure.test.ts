import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, test } from 'bun:test';

const dir = dirname(fileURLToPath(import.meta.url));
const readSource = (url: URL) => readFileSync(url, 'utf8');
const labSources = readdirSync(dir)
  .filter((name) => /\.(tsx|ts)$/.test(name) && !name.includes('.test.'))
  .map((name) => readFileSync(join(dir, name), 'utf8'))
  .join('\n');

describe('agent eval lab', () => {
  test('gates the page and sider entry on developer mode', () => {
    const page = readSource(new URL('./index.tsx', import.meta.url));
    const sider = readSource(new URL('../../components/layout/Sider/index.tsx', import.meta.url));
    const entry = readSource(
      new URL('../../components/layout/Sider/SiderNav/SiderEvalEntry.tsx', import.meta.url)
    );
    const router = readSource(new URL('../../components/layout/Router.tsx', import.meta.url));

    expect(page.includes('useDeveloperModeGate')).toBe(true);
    expect(page.includes("Navigate to='/guid'")).toBe(true);
    expect(page.includes('/api/debug/agent-evals')).toBe(false);
    expect(page.includes('evalApi.importPack')).toBe(true);
    expect(page.includes('getRunReport')).toBe(true);
    expect(page.includes('BusinessReportPanel')).toBe(true);
    expect(labSources.includes('exportBusinessReport')).toBe(true);
    expect(labSources.includes('eval.report.stopGuide')).toBe(true);
    expect(page.includes('downloadBusinessReportCsv')).toBe(false);
    expect(page.includes('eval.importNotes')).toBe(true);
    expect(page.includes('eval.trialsLocked')).toBe(true);
    expect(page.includes("useState('office_core')")).toBe(true);
    expect(page.includes('eval-task-profile')).toBe(true);
    expect(page.includes('eval.taskProfile.hint')).toBe(true);
    expect(page.includes('eval.runWithProfile')).toBe(true);
    expect(page.includes('conversation.taskProfile.office')).toBe(true);
    expect(page.includes('conversation.taskProfile.coding')).toBe(true);
    expect(page.includes('evalApi.startRun')).toBe(true);
    expect(page.includes('evalApi.cancelRun')).toBe(true);
    expect(page.includes('evalApi.history')).toBe(true);
    expect(page.includes('evalApi.diffRuns')).toBe(true);
    expect(page.includes('n_trials')).toBe(true);
    expect(page.includes('pass_at_1')).toBe(true);
    expect(page.includes('requires_sandbox')).toBe(true);
    expect(labSources.includes('reportTurn')).toBe(true);
    expect(labSources.includes('getCaseTrace')).toBe(true);
    expect(labSources.includes('getCaseObservation')).toBe(true);
    expect(page.includes('current_trace')).toBe(true);
    expect(page.includes('conversation_id')).toBe(true);
    expect(page.includes('workspace_label')).toBe(true);
    expect(page.includes('TraceView')).toBe(true);
    expect(page.includes('SegmentedTabs')).toBe(true);
    expect(page.includes('EvalCaseDetail')).toBe(true);
    expect(page.includes('eval.panel.run')).toBe(true);
    expect(page.includes('eval.officeval.note')).toBe(true);
    expect(labSources.includes('omegause_officeval')).toBe(true);
    expect(sider.includes('SiderEvalEntry')).toBe(true);
    expect(sider.includes('useDeveloperModeGate')).toBe(true);
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
    expect(api.includes('/history')).toBe(true);
    expect(api.includes('/diff/')).toBe(true);
    expect(api.includes('report-case')).toBe(true);
    expect(api.includes('private/sync')).toBe(true);
    expect(api.includes('/packs/import')).toBe(true);
    expect(api.includes('/report')).toBe(true);
    expect(api.includes('/observation')).toBe(true);
    expect(api.includes('getCaseTrace')).toBe(true);
    expect(api.includes('getCaseObservation')).toBe(true);
  });

  test('saves the business report through the desktop file APIs', () => {
    const csv = readSource(new URL('./businessReportExport.ts', import.meta.url));
    expect(csv.includes('ipcBridge.dialog.showSave')).toBe(true);
    expect(csv.includes('ipcBridge.fs.writeFile')).toBe(true);
    expect(csv.includes('buildBusinessReportHtml')).toBe(true);
    expect(csv.includes('createObjectURL')).toBe(false);
    expect(csv.includes('anchor.click()')).toBe(false);
  });
});
