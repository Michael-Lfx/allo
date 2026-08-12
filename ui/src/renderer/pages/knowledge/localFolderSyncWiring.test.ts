import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const bridgeSource = readFileSync(new URL('../../../common/adapter/ipcBridge.ts', import.meta.url), 'utf8');
const createStudioSource = readFileSync(new URL('./CreateStudio/index.tsx', import.meta.url), 'utf8');
const quickCaptureSource = readFileSync(new URL('./QuickCapture.tsx', import.meta.url), 'utf8');
const emptyStateSource = readFileSync(new URL('./KnowledgeEmptyState.tsx', import.meta.url), 'utf8');
const detailSource = readFileSync(new URL('./KnowledgeDetailPage/index.tsx', import.meta.url), 'utf8');
const zhLocale = JSON.parse(readFileSync(new URL('../../services/i18n/locales/zh-CN/knowledge.json', import.meta.url), 'utf8')) as Record<string, unknown>;
const enLocale = JSON.parse(readFileSync(new URL('../../services/i18n/locales/en-US/knowledge.json', import.meta.url), 'utf8')) as Record<string, unknown>;

describe('local-folder document sync wiring', () => {
  test('exposes the local-sync summary and both HTTP operations through the bridge', () => {
    expect(bridgeSource.includes('interface IKnowledgeLocalSyncSummary')).toBe(true);
    expect(bridgeSource.includes('getLocalSync: httpGet')).toBe(true);
    expect(bridgeSource.includes('syncLocalFolder: httpPost')).toBe(true);
    expect(bridgeSource.includes('/local-sync')).toBe(true);
  });

  test('keeps initial sync in the backend creation transaction', () => {
    expect(createStudioSource.includes("sourceType === 'local'")).toBe(true);
    expect(createStudioSource.includes('syncLocalFolder.invoke')).toBe(false);
    expect(quickCaptureSource.includes('syncLocalFolder.invoke')).toBe(false);
    expect(emptyStateSource.includes('syncLocalFolder.invoke')).toBe(false);
  });

  test('shows a manual sync control and per-file errors on local knowledge bases', () => {
    expect(detailSource.includes('getLocalSync')).toBe(true);
    expect(detailSource.includes('handleSyncLocalFolder')).toBe(true);
    expect(detailSource.includes('localSync.errors.map')).toBe(true);
  });

  test('keeps local-folder wording and sync status available in both locales', () => {
    const zhKnowledge = zhLocale as { localSync?: unknown; studio?: Record<string, unknown>; quick?: Record<string, unknown> };
    const enKnowledge = enLocale as { localSync?: unknown; studio?: Record<string, unknown>; quick?: Record<string, unknown> };
    expect(zhKnowledge.localSync).toBeDefined();
    expect(enKnowledge.localSync).toBeDefined();
    expect(zhKnowledge.studio?.localTeachHow).toBeDefined();
    expect(enKnowledge.studio?.localTeachHow).toBeDefined();
    expect(zhKnowledge.quick?.localDesc).toBeDefined();
    expect(enKnowledge.quick?.localDesc).toBeDefined();
  });
});
