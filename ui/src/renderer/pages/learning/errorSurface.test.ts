/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 错误面单源改造的契约测试（o1）：errorMessage 按 status/传输类别映射
 * 学习域词典（原文一律不上 UI，只进 console）；新 key 双语对称（红线 2，
 * 遍历真实词典 JSON）；学习模块源面负向断言（错误原文/强弹窗/拼接直泄
 * 不再出现——收编后的回归栅栏）。 */
import { describe, expect, test } from 'bun:test';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createInstance } from 'i18next';
import { initReactI18next } from 'react-i18next';

import { BackendHttpError, BackendRequestError } from '@/common/adapter/httpBridge';
import learningEn from '@/renderer/services/i18n/locales/en-US/learning.json';
import learningZh from '@/renderer/services/i18n/locales/zh-CN/learning.json';
import { errorMessage, type Translate } from './utils';

/** 本票新增/改词的词典 key（人面固定句 = 独立事实源，票面措辞直书）。 */
const NEW_KEYS: Record<string, { zh: string; en: string }> = {
  conflictBusy: {
    zh: '当前有任务正在进行，请稍后再试',
    en: 'Another task is currently running. Please try again later.',
  },
  contentMissing: {
    zh: '内容不存在或已删除',
    en: 'This content does not exist or has been deleted.',
  },
  serviceUnavailable: {
    zh: '服务暂时不可用，请稍后再试',
    en: 'The service is temporarily unavailable. Please try again later.',
  },
  networkUnstable: {
    zh: '网络不稳定，请稍后再试',
    en: 'The network is unstable. Please try again later.',
  },
  actionFailed: {
    zh: '操作失败，请稍后再试',
    en: 'The action failed. Please try again later.',
  },
  courseInfoLoadFailed: {
    zh: '课程信息读取失败',
    en: 'Failed to load course information.',
  },
  growthPillFailedHint: {
    zh: '执行失败，悬停可查看详情',
    en: 'Failed — hover for details',
  },
  systemActionReasonTimeout: { zh: '处理超时', en: 'Processing timed out.' },
  systemActionReasonConflict: {
    zh: '有同类任务正在进行',
    en: 'A similar task is already running.',
  },
  systemActionReasonInvalidOutput: {
    zh: '生成结果未通过校验',
    en: 'The generated result failed validation.',
  },
  systemActionReasonBusy: { zh: '服务繁忙', en: 'The service is busy.' },
  // F6 语义修正新增两档：not implemented / internal error 不再借用上面两档。
  systemActionReasonUnsupported: {
    zh: '暂不支持的操作',
    en: "This action isn't supported yet.",
  },
  systemActionReasonServiceUnavailable: {
    zh: '服务暂时不可用',
    en: 'The service is temporarily unavailable.',
  },
  systemActionReasonUnknown: { zh: '未知原因', en: 'Unknown reason.' },
};

const initTestI18n = async (locale: 'zh-CN' | 'en-US') => {
  const instance = createInstance();
  await instance.use(initReactI18next).init({
    lng: locale,
    fallbackLng: 'zh-CN',
    resources: {
      'zh-CN': { translation: { learning: learningZh } },
      'en-US': { translation: { learning: learningEn } },
    },
    interpolation: { escapeValue: false },
  });
  return instance;
};

const tOf = async (locale: 'zh-CN' | 'en-US'): Promise<Translate> => {
  const instance = await initTestI18n(locale);
  return instance.t.bind(instance) as unknown as Translate;
};

const httpError = (status: number, body: unknown = { error: 'raw backend detail', code: 'X' }) =>
  new BackendHttpError({ method: 'POST', path: '/api/learning/some-endpoint', status, body });

describe('errorMessage 单源映射矩阵', () => {
  test('409 → 通用冲突句（d7 拦截路径保留既有友好文案，零改动）', async () => {
    const zh = await tOf('zh-CN');
    const en = await tOf('en-US');
    expect(errorMessage(zh, httpError(409))).toBe(NEW_KEYS.conflictBusy.zh);
    expect(errorMessage(en, httpError(409))).toBe(NEW_KEYS.conflictBusy.en);
  });

  test('404 → 「内容不存在或已删除」', async () => {
    const zh = await tOf('zh-CN');
    const en = await tOf('en-US');
    expect(errorMessage(zh, httpError(404))).toBe(NEW_KEYS.contentMissing.zh);
    expect(errorMessage(en, httpError(404))).toBe(NEW_KEYS.contentMissing.en);
  });

  test('5xx → 「服务暂时不可用，请稍后再试」', async () => {
    const zh = await tOf('zh-CN');
    const en = await tOf('en-US');
    for (const status of [500, 502, 503]) {
      expect(errorMessage(zh, httpError(status))).toBe(NEW_KEYS.serviceUnavailable.zh);
      expect(errorMessage(en, httpError(status))).toBe(NEW_KEYS.serviceUnavailable.en);
    }
  });

  test('其余 4xx → 通用失败句', async () => {
    const zh = await tOf('zh-CN');
    for (const status of [400, 401, 403, 429]) {
      expect(errorMessage(zh, httpError(status))).toBe(NEW_KEYS.actionFailed.zh);
    }
  });

  test('网络/超时（BackendRequestError）→ 「网络不稳定，请稍后再试」', async () => {
    const zh = await tOf('zh-CN');
    const en = await tOf('en-US');
    for (const kind of ['network', 'timeout', 'aborted'] as const) {
      const error = new BackendRequestError(kind, `Backend POST /api/x ${kind} (raw detail)`);
      expect(errorMessage(zh, error)).toBe(NEW_KEYS.networkUnstable.zh);
      expect(errorMessage(en, error)).toBe(NEW_KEYS.networkUnstable.en);
    }
  });

  test('未知 Error / 非 Error 值 → 通用失败句；原文一律不出现在映射结果', async () => {
    const zh = await tOf('zh-CN');
    const en = await tOf('en-US');
    const raw = new Error('ECONNRESET raw stack: [gd.terrain.keys] 未知顶层键「type」');
    expect(errorMessage(zh, raw)).toBe(NEW_KEYS.actionFailed.zh);
    expect(errorMessage(zh, raw)).not.toContain('gd.terrain');
    expect(errorMessage(en, raw)).toBe(NEW_KEYS.actionFailed.en);
    expect(errorMessage(zh, '字符串错误')).toBe(NEW_KEYS.actionFailed.zh);
    expect(errorMessage(zh, null)).toBe(NEW_KEYS.actionFailed.zh);
    expect(errorMessage(zh, undefined)).toBe(NEW_KEYS.actionFailed.zh);
  });
});

describe('新词典 key 双语对称（红线 2，真实词典 JSON 遍历）', () => {
  test('新增 key 在 zh-CN 与 en-US 均为票面固定句', () => {
    const zh = learningZh as Record<string, unknown>;
    const en = learningEn as Record<string, unknown>;
    for (const [name, expected] of Object.entries(NEW_KEYS)) {
      expect(zh[name]).toBe(expected.zh);
      expect(en[name]).toBe(expected.en);
    }
  });

  test('F2：五个动作失败 toast 标题 key 双语在场且 zh 侧两两可分辨', () => {
    // o1 降级强弹窗时删了标题 key，五处失败 toast 同文分不清动作；F2 恢复。
    const titleKeys = [
      'learningGraphStructureFailed',
      'learningGraphMountFailed',
      'learningGraphRepairFailed',
      'learningGraphBasisDeclareFailed',
      'learningGraphGrowFailed',
    ] as const;
    const zh = learningZh as Record<string, unknown>;
    const en = learningEn as Record<string, unknown>;
    const zhValues: string[] = [];
    for (const name of titleKeys) {
      expect(typeof zh[name]).toBe('string');
      expect((zh[name] as string).length).toBeGreaterThan(0);
      expect(typeof en[name]).toBe('string');
      expect((en[name] as string).length).toBeGreaterThan(0);
      zhValues.push(zh[name] as string);
    }
    // 可分辨性：zh 标题两两不同（同文 = 评审裁决点名的病）。
    expect(new Set(zhValues).size).toBe(titleKeys.length);
  });

  test('zh-CN 与 en-US 的 learning 词典 key 集合完全一致', () => {
    const zh = Object.keys(learningZh as Record<string, unknown>).sort();
    const en = Object.keys(learningEn as Record<string, unknown>).sort();
    expect(en).toEqual(zh);
  });
});

/** 学习模块源面负向断言（回归栅栏）：直泄惯用法逐 pattern 归零。 */
const LEARNING_DIR = fileURLToPath(new URL('.', import.meta.url));
const FORBIDDEN: Array<{ pattern: RegExp; why: string }> = [
  // 强制弹窗直泄已收编为非阻塞 toast（c2 口径，与同文件其它动作一致）。
  { pattern: /\bModal\.error\(/, why: 'Modal.error 强弹窗' },
  // error.message 直用上屏（setter/Message）——检测语义用 .message 仍允许在 utils.ts。
  {
    pattern: /instanceof Error \? \w+\.message/,
    why: 'error.message 直用（utils.ts 单源之外）',
  },
  // 首屏拼接：`loadFailed: ${error}` 把原始报文接进 Alert。
  { pattern: /loadFailed'\)}: /, why: 'loadFailed 拼接原始 error' },
  // 系统动作簿失败原因原文直出（主显示改走 reasonKey 分类映射）。原文现以
  // `reasonDetail` 字段随行模型下传、只经 Tooltip 收纳（接线契约见
  // systemActions.test.ts）——裸 `row.reason` 仍是被禁的旧直出惯用法。
  { pattern: /\brow\.reason\b/, why: 'row.reason 原文直出' },
];
const EXEMPT_SOURCE_PATTERNS = new Set<string>(['utils.ts']);

const collectSources = (dir: string): string[] => {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    if (statSync(full).isDirectory()) {
      out.push(...collectSources(full));
      continue;
    }
    if (/\.(ts|tsx)$/.test(name) && !/\.test\.(ts|tsx)$/.test(name)) out.push(full);
  }
  return out;
};

describe('错误面源契约（收编后负向断言：原文不再上屏）', () => {
  test('学习模块源面不再出现直泄惯用法', () => {
    const offenders: string[] = [];
    for (const file of collectSources(LEARNING_DIR)) {
      const rel = file.replaceAll('\\', '/').split('/learning/')[1] ?? file;
      const source = readFileSync(file, 'utf8');
      for (const { pattern, why } of FORBIDDEN) {
        if (EXEMPT_SOURCE_PATTERNS.has(rel) && pattern.source.includes('message')) continue;
        if (pattern.test(source)) offenders.push(`${rel} → ${why}`);
      }
    }
    expect(offenders).toEqual([]);
  });
});
