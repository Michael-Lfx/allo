

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('Nomi session metrics panel notice', () => {
  test('renders a data reliability notice in the metrics panel', () => {
    const source = readSource(new URL('./NomiSessionMetricsPanel.tsx', import.meta.url));

    expect(source.includes('conversation.sessionMetrics.notice')).toBe(true);
  });

  test('uses the required Chinese notice copy', () => {
    const zh = JSON.parse(readSource(new URL('../../../../services/i18n/locales/zh-CN/conversation.json', import.meta.url)));

    expect(zh.sessionMetrics.notice).toBe('因数据采集手段问题，数据仅供参考，不可作为定论');
  });

  test('rehydrates persisted usage when the metrics tab mounts', () => {
    const source = readSource(new URL('./NomiSessionMetricsPanel.tsx', import.meta.url));

    expect(source.includes('getConversationOrNull')).toBe(true);
    expect(source.includes('getPersistedUsage(latest)')).toBe(true);
  });
});

describe('Nomi session metrics mounted capabilities', () => {
  test('panel reads the conversation mount snapshot', () => {
    const source = readSource(new URL('./NomiSessionMetricsPanel.tsx', import.meta.url));

    expect(source.includes('getMountedCapabilities')).toBe(true);
    expect(source.includes('hasMountedCapabilities')).toBe(true);
    expect(source.includes('conversation.sessionMetrics.mountedTitle')).toBe(true);
    expect(source.includes('conversation.sessionMetrics.mountedMcp')).toBe(true);
    expect(source.includes('conversation.sessionMetrics.mountedSkills')).toBe(true);
    expect(source.includes('conversation.sessionMetrics.mountedEmpty')).toBe(true);
  });

  test('defines mounted capability copy in both locales', () => {
    const zh = JSON.parse(
      readSource(new URL('../../../../services/i18n/locales/zh-CN/conversation.json', import.meta.url))
    );
    const en = JSON.parse(
      readSource(new URL('../../../../services/i18n/locales/en-US/conversation.json', import.meta.url))
    );

    expect(zh.sessionMetrics.mountedTitle).toBe('挂载能力');
    expect(zh.sessionMetrics.mountedMcp).toBe('MCP');
    expect(zh.sessionMetrics.mountedSkills).toBe('Skills');
    expect(zh.sessionMetrics.mountedEmpty).toBe('未挂载');
    expect(en.sessionMetrics.mountedTitle).toBe('Mounted');
    expect(en.sessionMetrics.mountedMcp).toBe('MCP');
    expect(en.sessionMetrics.mountedSkills).toBe('Skills');
    expect(en.sessionMetrics.mountedEmpty).toBe('None mounted');
  });
});

describe('conversation update body must not forward merge_extra', () => {
  test('ipcBridge conversation.update omits the client-only merge_extra flag', () => {
    const source = readSource(new URL('../../../../../common/adapter/ipcBridge.ts', import.meta.url));
    const updateStart = source.indexOf('update: httpPatch<boolean, { conversation_id: ConversationId;');
    expect(updateStart).toBeGreaterThan(-1);
    const updateChunk = source.slice(updateStart, updateStart + 700);

    expect(updateChunk.includes('deny_unknown_fields')).toBe(true);
    expect(updateChunk.includes('merge_extra: p.merge_extra')).toBe(false);
  });
});
