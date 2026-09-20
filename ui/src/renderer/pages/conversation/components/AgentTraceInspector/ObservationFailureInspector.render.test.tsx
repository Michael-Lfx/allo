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
import conversation from '@/renderer/services/i18n/locales/zh-CN/conversation.json';
import ObservationFailureInspector, { buildReproBundle } from './ObservationFailureInspector';
import type { ProjectedTurn } from './useAgentTraces';

const testI18n = createInstance();
await testI18n.use(initReactI18next).init({
  lng: 'zh-CN',
  fallbackLng: 'zh-CN',
  resources: { 'zh-CN': { translation: { conversation } } },
  interpolation: { escapeValue: false },
});

const baseTurn: ProjectedTurn = {
  root_turn_id: 'turn-1',
  conversation_id: 'conv-1',
  status: 'completed',
  integrity: 'complete',
  interrupted: false,
  gap_count: 0,
  timeline: [],
  model_calls: [],
  gaps: [],
};

const failedTurn: ProjectedTurn = {
  ...baseTurn,
  root_turn_id: 'turn_fail_1',
  status: 'failed',
  error: 'Network connection timeout to provider gateway',
  model_calls: [],
};

describe('ObservationFailureInspector render contract', () => {
  test('does not render when turn completed normally without errors', () => {
    const html = renderToStaticMarkup(
      <I18nextProvider i18n={testI18n}>
        <ObservationFailureInspector turn={baseTurn} />
      </I18nextProvider>
    );
    expect(html).toBe('');
  });

  test('renders preparation failure callout when turn fails before model calls', () => {
    const html = renderToStaticMarkup(
      <I18nextProvider i18n={testI18n}>
        <ObservationFailureInspector turn={failedTurn} />
      </I18nextProvider>
    );

    expect(html).toContain('故障诊断与失败分析');
    expect(html).toContain('准备阶段失败（模型调用前）');
    expect(html).toContain('Network connection timeout to provider gateway');
  });

  test('renders error box without preparation callout when model calls exist', () => {
    const failedAfterCallTurn: ProjectedTurn = {
      ...baseTurn,
      status: 'failed',
      error: 'Artifact delivery failed: disk quota exceeded',
      model_calls: [
        {
          model_call_id: 'call-1',
          interrupted: false,
          tools: [],
        },
      ],
    };
    const html = renderToStaticMarkup(
      <I18nextProvider i18n={testI18n}>
        <ObservationFailureInspector turn={failedAfterCallTurn} />
      </I18nextProvider>
    );

    expect(html).toContain('故障诊断与失败分析');
    expect(html).not.toContain('准备阶段失败（模型调用前）');
    expect(html).toContain('Artifact delivery failed: disk quota exceeded');
  });

  test('renders gap errors when observation gap has error payload', () => {
    const turnWithGapError: ProjectedTurn = {
      ...baseTurn,
      gap_count: 1,
      gaps: [
        {
          event_seq: 42,
          reason: 'provider_stream_failed',
          error: 'Rate limit exceeded: 429 Too Many Requests',
        },
      ],
    };
    const html = renderToStaticMarkup(
      <I18nextProvider i18n={testI18n}>
        <ObservationFailureInspector turn={turnWithGapError} />
      </I18nextProvider>
    );

    expect(html).toContain('故障诊断与失败分析');
    expect(html).toContain('伴随观测缺口 (Gap)');
    expect(html).toContain('事件 42');
    expect(html).toContain('provider_stream_failed');
    expect(html).toContain('Rate limit exceeded: 429 Too Many Requests');
  });

  test('ignores whitespace-only error string and does not render when completed', () => {
    const whitespaceTurn: ProjectedTurn = {
      ...baseTurn,
      error: '   \n  \t  ',
    };
    const html = renderToStaticMarkup(
      <I18nextProvider i18n={testI18n}>
        <ObservationFailureInspector turn={whitespaceTurn} />
      </I18nextProvider>
    );
    expect(html).toBe('');
  });

  test('does not render when interrupted normally without error', () => {
    const interruptedTurn: ProjectedTurn = {
      ...baseTurn,
      status: 'interrupted',
      interrupted: true,
      error: null,
    };
    const html = renderToStaticMarkup(
      <I18nextProvider i18n={testI18n}>
        <ObservationFailureInspector turn={interruptedTurn} />
      </I18nextProvider>
    );
    expect(html).toBe('');
  });

  test('renders raw payload section when status is failed even if error is absent', () => {
    const failedNoMsgTurn: ProjectedTurn = {
      ...baseTurn,
      status: 'failed',
      error: null,
      model_calls: [
        {
          model_call_id: 'call-1',
          interrupted: false,
          tools: [],
        },
      ],
    };
    const html = renderToStaticMarkup(
      <I18nextProvider i18n={testI18n}>
        <ObservationFailureInspector turn={failedNoMsgTurn} />
      </I18nextProvider>
    );
    expect(html).toContain('故障诊断与失败分析');
    expect(html).not.toContain('准备阶段失败（模型调用前）');
    expect(html).toContain('技术错误载荷 (Raw Payload)');
  });

  test('renders gap error but skips preparation callout when completed turn has no model calls', () => {
    const completedTurnWithGapError: ProjectedTurn = {
      ...baseTurn,
      status: 'completed',
      error: null,
      model_calls: [],
      gap_count: 1,
      gaps: [
        {
          event_seq: 3,
          reason: 'buffer_overflow',
          error: 'Connection dropped during observation',
          from_seq: 1,
          to_seq: 2,
        },
      ],
    };
    const html = renderToStaticMarkup(
      <I18nextProvider i18n={testI18n}>
        <ObservationFailureInspector turn={completedTurnWithGapError} />
      </I18nextProvider>
    );
    expect(html).toContain('故障诊断与失败分析');
    expect(html).not.toContain('准备阶段失败（模型调用前）');
    expect(html).toContain('Connection dropped during observation');
    expect(html).toContain('伴随观测缺口 (Gap)');
  });

  test('builds a structured repro bundle with turn context and environment', () => {
    const bundle = buildReproBundle(failedTurn);
    expect(bundle.schema_version).toBe(1);
    expect((bundle.turn as { root_turn_id: string }).root_turn_id).toBe('turn_fail_1');
    expect((bundle.turn as { error: string }).error).toBe('Network connection timeout to provider gateway');
    expect(typeof bundle.generated_at_ms).toBe('number');
  });

  test('renders the repro copy button with proper tooltip and aria-label', () => {
    const html = renderToStaticMarkup(
      <I18nextProvider i18n={testI18n}>
        <ObservationFailureInspector turn={failedTurn} />
      </I18nextProvider>
    );
    expect(html).toContain('aria-label="复制复现排障包 (Repro JSON)"');
  });
});
