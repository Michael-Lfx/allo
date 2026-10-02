/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { createInstance } from 'i18next';
import React from 'react';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { renderToStaticMarkup } from 'react-dom/server';
import enConversation from '@/renderer/services/i18n/locales/en-US/conversation.json';
import zhConversation from '@/renderer/services/i18n/locales/zh-CN/conversation.json';
import { formatBuildBrief, formatBuildFull } from './buildInfo';
import ToolPhaseBreakdown from './ToolPhaseBreakdown';
import { formatPhaseDuration, phaseLabelKey, toolPhaseBreakdown } from './toolPhases';
import type { ObservationBuildInfo, ProjectedToolExecution } from './useAgentTraces';

const testI18n = createInstance();
await testI18n.use(initReactI18next).init({
  lng: 'zh-CN',
  fallbackLng: 'zh-CN',
  resources: { 'zh-CN': { translation: { conversation: zhConversation } } },
  interpolation: { escapeValue: false },
});

function tool(completed: unknown): ProjectedToolExecution {
  return { tool_call_id: 'call-1', name: 'Bash', status: 'completed', completed };
}

const bashCompleted = {
  duration_us: 1_300_000,
  phases: [
    { name: 'bash.prepare', us: 400 },
    { name: 'bash.spawn', us: 52_000 },
    { name: 'bash.wait', us: 1_150_000 },
    { name: 'bash.render', us: 900 },
    { name: 'tool.execute', us: 1_204_000 },
    { name: 'hooks.pre', us: 300 },
    { name: 'hooks.post', us: 50 },
    { name: 'tool.post_process', us: 2_000 },
  ],
};

describe('toolPhaseBreakdown', () => {
  test('is null for traces without phases', () => {
    expect(toolPhaseBreakdown(tool({ duration_us: 5 }))).toBeNull();
    expect(toolPhaseBreakdown(tool(undefined))).toBeNull();
    expect(toolPhaseBreakdown(tool({ phases: [{ name: 'x' }, 'bad', { name: 3, us: 1 }] }))).toBeNull();
  });

  test('orders stages and nests detail phases under tool.execute', () => {
    const breakdown = toolPhaseBreakdown(tool(bashCompleted));
    expect(breakdown?.rows.map((row) => [row.name, row.nested])).toEqual([
      ['hooks.pre', false],
      ['tool.execute', false],
      ['bash.prepare', true],
      ['bash.spawn', true],
      ['bash.wait', true],
      ['bash.render', true],
      ['tool.post_process', false],
      ['hooks.post', false],
    ]);
    expect(breakdown?.totalUs).toBe(1_300_000);
    const wait = breakdown?.rows.find((row) => row.name === 'bash.wait');
    expect(wait?.share).toBeCloseTo(1_150_000 / 1_300_000, 5);
  });

  test('reads phases from failed executions too', () => {
    const failed: ProjectedToolExecution = {
      tool_call_id: 'call-2',
      status: 'failed',
      failed: { phases: [{ name: 'tool.execute', us: 10 }] },
    };
    expect(toolPhaseBreakdown(failed)?.rows).toHaveLength(1);
  });

  test('formats durations at the right scale', () => {
    expect(formatPhaseDuration(400)).toBe('400µs');
    expect(formatPhaseDuration(1_500)).toBe('1.5ms');
    expect(formatPhaseDuration(52_000)).toBe('52ms');
    expect(formatPhaseDuration(1_204_000)).toBe('1.2s');
    expect(formatPhaseDuration(2_000_000)).toBe('2s');
    expect(formatPhaseDuration(12_340_000)).toBe('12.3s');
  });
});

describe('ToolPhaseBreakdown render', () => {
  test('renders localized labels and durations', () => {
    const html = renderToStaticMarkup(
      <I18nextProvider i18n={testI18n}>
        <ToolPhaseBreakdown tool={tool(bashCompleted)} />
      </I18nextProvider>
    );
    expect(html).toContain('耗时分解');
    expect(html).toContain('等待退出');
    expect(html).toContain('1.15s');
    expect(html).toContain('is-nested');
  });

  test('renders nothing without phases', () => {
    const html = renderToStaticMarkup(
      <I18nextProvider i18n={testI18n}>
        <ToolPhaseBreakdown tool={tool({ duration_us: 5 })} />
      </I18nextProvider>
    );
    expect(html).toBe('');
  });

  test('falls back to the raw name for an unknown phase', () => {
    const html = renderToStaticMarkup(
      <I18nextProvider i18n={testI18n}>
        <ToolPhaseBreakdown tool={tool({ phases: [{ name: 'mcp.roundtrip', us: 5 }] })} />
      </I18nextProvider>
    );
    expect(html).toContain('mcp.roundtrip');
  });
});

describe('phase and build locale coverage', () => {
  const phaseNames = [
    'hooks.pre',
    'tool.execute',
    'tool.post_process',
    'hooks.post',
    'bash.prepare',
    'bash.spawn',
    'bash.wait',
    'bash.render',
    'exec.prepare',
    'exec.spawn',
    'exec.wait',
  ];

  test('every phase has a label in both locales', () => {
    for (const locale of [zhConversation, enConversation]) {
      const agentTrace = locale.agentTrace as Record<string, string>;
      for (const name of phaseNames) {
        const key = phaseLabelKey(name).replace('conversation.agentTrace.', '');
        expect(agentTrace[key]).toBeTruthy();
      }
      for (const key of ['buildInfo', 'buildDirty', 'buildUnrecorded', 'phaseTitle']) {
        expect(agentTrace[key]).toBeTruthy();
      }
    }
  });
});

describe('build info formatting', () => {
  const build: ObservationBuildInfo = {
    app_version: '1.5.3',
    git_sha: '0123456789ab',
    git_dirty: true,
    profile: 'release',
    build_time: '1790000000',
    os: 'windows',
    arch: 'x86_64',
  };

  test('brief shows version, short sha and the dirty marker', () => {
    expect(formatBuildBrief(build, 'dirty')).toBe('v1.5.3 · 0123456 · dirty');
    expect(formatBuildBrief({ ...build, git_dirty: false }, 'dirty')).toBe('v1.5.3 · 0123456');
  });

  test('brief degrades to the version when the commit is unknown', () => {
    expect(formatBuildBrief({ ...build, git_sha: 'unknown' }, 'dirty')).toBe('v1.5.3');
    expect(formatBuildBrief({ app_version: '1.5.3', os: 'linux', arch: 'aarch64' }, 'dirty')).toBe('v1.5.3');
  });

  test('full keeps the whole sha, profile and platform', () => {
    const full = formatBuildFull(build, 'dirty');
    expect(full).toContain('v1.5.3 · 0123456789ab (dirty) · release');
    expect(full.endsWith('windows/x86_64')).toBe(true);
  });
});
