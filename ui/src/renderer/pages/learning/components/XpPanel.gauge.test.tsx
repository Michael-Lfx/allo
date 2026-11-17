/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

// c1/R-b + R-d 渲染契约：进度尺读数行——分母剔 skipped 后全跳课程走
// 「无可计节点」分支（f2 联动）；ETA 正常臂披露步伐基数、低步伐降级臂
// 不直出天数。SSR 真渲染（bun test 无 DOM），i18n 双 locale 各断言一臂。
import { describe, expect, test } from 'bun:test';
import { createInstance } from 'i18next';
import React from 'react';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { renderToStaticMarkup } from 'react-dom/server';

import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';
import type { XpReading } from '../types';
import { XpGaugeReadout } from './XpPanel';

function makeI18n(lng: 'zh-CN' | 'en-US') {
  const instance = createInstance();
  void instance.use(initReactI18next).init({
    lng,
    fallbackLng: lng,
    resources: {
      'zh-CN': { translation: { learning: learningZh } },
      'en-US': { translation: { learning: learningEn } },
    },
    interpolation: { escapeValue: false },
  });
  return instance;
}

function reading(overrides: Partial<XpReading>): XpReading {
  return {
    review_day: 20261117,
    today_xp: 0,
    goal: 40,
    goal_achieved: false,
    settled_nodes: 0,
    total_nodes: 4,
    remaining_est: 100,
    eta_days: null,
    pace_xp: 0,
    ...overrides,
  };
}

const renderGauge = (reading: XpReading, lng: 'zh-CN' | 'en-US' = 'zh-CN'): string =>
  renderToStaticMarkup(
    <I18nextProvider i18n={makeI18n(lng)}>
      <XpGaugeReadout reading={reading} />
    </I18nextProvider>
  );

describe('XpGaugeReadout 契约（c1：分母剔 skipped + ETA 语境）', () => {
  test('正常臂（zh-CN）：0/4 分母与剩余 est 对上课程卡，ETA 披露步伐基数', () => {
    const html = renderGauge(
      reading({ settled_nodes: 1, total_nodes: 4, remaining_est: 100, eta_days: 2, pace_xp: 5 })
    );
    expect(html).toContain('已结算 1/4 节点');
    expect(html).toContain('剩余 100 est 分钟');
    expect(html).toContain('现步伐约 2 天学完 · 按近 7 天均 5 XP');
  });

  test('低步伐降级臂（zh-CN）：不直出天数，改「暂无步伐」', () => {
    const html = renderGauge(
      reading({ total_nodes: 4, remaining_est: 100, eta_days: null, pace_xp: 2 })
    );
    expect(html).toContain('暂无步伐，开始学习后可估算');
    expect(html).not.toContain('天学完');
    expect(html).not.toContain('按近 7 天均');
    // 分母读数不受降级影响。
    expect(html).toContain('已结算 0/4 节点');
  });

  test('全跳分支（zh-CN，f2 联动）：无可计节点，不渲染 0/0 与剩余噪声', () => {
    const html = renderGauge(
      reading({ settled_nodes: 0, total_nodes: 0, remaining_est: 0, eta_days: null, pace_xp: 0 })
    );
    expect(html).toContain('无可计节点');
    expect(html).not.toContain('已结算 0/0');
    expect(html).not.toContain('est 分钟');
  });

  test('正常臂（en-US）：双 locale 对称，步伐披露同在', () => {
    const html = renderGauge(
      reading({ settled_nodes: 1, total_nodes: 4, remaining_est: 100, eta_days: 2, pace_xp: 5 }),
      'en-US'
    );
    expect(html).toContain('1/4 nodes settled');
    expect(html).toContain('100 est minutes left');
    expect(html).toContain('~2 days to finish at current pace · 5 XP/day over the last 7 days');
  });

  test('低步伐降级臂（en-US）：无天数直出', () => {
    const html = renderGauge(
      reading({ eta_days: null, pace_xp: 2 }),
      'en-US'
    );
    expect(html).toContain('No pace yet');
    expect(html).not.toContain('days to finish');
  });
});
