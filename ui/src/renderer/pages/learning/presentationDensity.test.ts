/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** o6 呈现密度收口的词典断言：packContract 双语删除（首屏架构词 Alert
 * 退役）、新增收纳面 key 双语对称、复制详情复用既有 common 复制反馈词典
 * （不新造反馈文案）。零渲染——只读词典 JSON（settingsCopyPlain.test.ts
 * 同款 seam）；期望值来自票面裁决，不重算实现。 */
import { describe, expect, test } from 'bun:test';

import commonEn from '@/renderer/services/i18n/locales/en-US/common.json';
import commonZh from '@/renderer/services/i18n/locales/zh-CN/common.json';
import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';

type LocaleJson = Record<string, unknown>;

const zh = learningZh as unknown as LocaleJson;
const en = learningEn as unknown as LocaleJson;
const commonZhDict = commonZh as unknown as LocaleJson;
const commonEnDict = commonEn as unknown as LocaleJson;

describe('o6 呈现密度词典断言', () => {
  test('packContract 双语删除：架构词不再进用户词典（键与文案一并退役）', () => {
    expect(zh.packContract).toBeUndefined();
    expect(en.packContract).toBeUndefined();
  });

  test('新增收纳面 key 双语对称、非空、无插值占位（折叠头为纯词）', () => {
    // 三个收纳面折叠头：挂载条（结构图 Tab）、来源覆盖卡、失败错误详情、
    // triage 明细（F4 收进残余计数的折叠，同「详情」口径）。
    const keys = [
      'learningGraphMountDetails',
      'sourceCoverageDetails',
      'genTriageDetails',
      'genFailedDetails',
      'genFailedCopyDetails',
    ];
    for (const key of keys) {
      expect(typeof zh[key]).toBe('string');
      expect(typeof en[key]).toBe('string');
      expect((zh[key] as string).length).toBeGreaterThan(0);
      expect((en[key] as string).length).toBeGreaterThan(0);
      expect((zh[key] as string).includes('{{')).toBe(false);
      expect((en[key] as string).includes('{{')).toBe(false);
    }
    // 收纳面折叠头统一「详情」口径（同一词，双语各自一致）。
    expect(zh.learningGraphMountDetails).toBe(zh.sourceCoverageDetails);
    expect(en.learningGraphMountDetails).toBe(en.sourceCoverageDetails);
  });

  test('复制详情不新造反馈文案：成功/失败复用 common.copySuccess/copyFailed', () => {
    expect(typeof commonZhDict.copySuccess).toBe('string');
    expect(typeof commonEnDict.copySuccess).toBe('string');
    expect(typeof commonZhDict.copyFailed).toBe('string');
    expect(typeof commonEnDict.copyFailed).toBe('string');
    // 动作键只出动作名，不内嵌结果文案。
    expect(zh.genFailedCopyDetails).not.toContain('已复制');
    expect(zh.genFailedCopyDetails).not.toContain('复制失败');
    expect(en.genFailedCopyDetails).not.toContain('Copied');
    expect(en.genFailedCopyDetails).not.toContain('Copy failed');
  });
});
