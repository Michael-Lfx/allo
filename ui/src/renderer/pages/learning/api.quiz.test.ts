/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** learningApi quiz 四端点（u1 接线层）的 HTTP 边界观察：逐用例替换
 * globalThis.fetch 截获方法/路径/请求体并还原（api.declare-basis.test.ts
 * 同款纪律）。契约面（routes.rs 912–995）：progress = GET 读数；start =
 * POST 无请求体（慢路径 = 变式链 LLM，后端内联生成）；grade = POST
 * `{activity_id, response}`（纯读零落账）；complete = POST `{answers}`
 * （必作答，缺题拒绝）。校准读面 /quiz/calibration 本波不接（u4 票面
 * 裁决）——以 learningApi 无 quizCalibration 钉住节制，防未裁决先接线。 */
import { afterEach, describe, expect, test } from 'bun:test';
import { learningApi } from './api';

interface CapturedRequest {
  method: string;
  url: string;
  body: unknown;
}

const realFetch = globalThis.fetch;

function captureRequests(): CapturedRequest[] {
  const captured: CapturedRequest[] = [];
  globalThis.fetch = ((_url: string, init?: RequestInit) => {
    captured.push({
      method: String(init?.method),
      url: String(_url),
      body: init?.body === undefined ? undefined : JSON.parse(String(init?.body)),
    });
    return Promise.resolve(
      new Response(JSON.stringify({ success: true, data: { unwrapped: true } }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })
    );
  }) as unknown as typeof fetch;
  return captured;
}

afterEach(() => {
  globalThis.fetch = realFetch;
});

describe('learningApi quiz endpoints (u1)', () => {
  test('quizProgress = GET …/quiz/progress，零请求体', async () => {
    const captured = captureRequests();

    const result = await learningApi.quizProgress('course-1');

    expect(result).toEqual({ unwrapped: true });
    expect(captured).toHaveLength(1);
    expect(captured[0]?.method).toBe('GET');
    expect(captured[0]?.url).toContain('/api/learning/courses/course-1/quiz/progress');
    expect(captured[0]?.body).toBeUndefined();
  });

  test('quizStart = POST …/quiz/start，无请求体（后端零读取）', async () => {
    const captured = captureRequests();

    const result = await learningApi.quizStart('course-1');

    expect(result).toEqual({ unwrapped: true });
    expect(captured).toHaveLength(1);
    expect(captured[0]?.method).toBe('POST');
    expect(captured[0]?.url).toContain('/api/learning/courses/course-1/quiz/start');
    expect(captured[0]?.body).toBeUndefined();
  });

  test('quizGrade = POST …/rounds/{round_id}/grade，请求体 {activity_id, response}', async () => {
    const captured = captureRequests();
    const response = { value: 'A' };

    const result = await learningApi.quizGrade('course-1', 'round-9', 'act-1', response);

    expect(result).toEqual({ unwrapped: true });
    expect(captured).toHaveLength(1);
    expect(captured[0]?.method).toBe('POST');
    expect(captured[0]?.url).toContain('/api/learning/courses/course-1/quiz/rounds/round-9/grade');
    expect(captured[0]?.body).toEqual({ activity_id: 'act-1', response });
  });

  test('quizComplete = POST …/rounds/{round_id}/complete，请求体 {answers}（必作答全卷）', async () => {
    const captured = captureRequests();
    const answers = { 'act-1': { value: 'A' }, 'act-2': '甲' };

    const result = await learningApi.quizComplete('course-1', 'round-9', answers);

    expect(result).toEqual({ unwrapped: true });
    expect(captured).toHaveLength(1);
    expect(captured[0]?.method).toBe('POST');
    expect(captured[0]?.url).toContain(
      '/api/learning/courses/course-1/quiz/rounds/round-9/complete'
    );
    expect(captured[0]?.body).toEqual({ answers });
  });

  test('路径片段 encodeURIComponent：带特殊字符的 id 不破路由', async () => {
    const captured = captureRequests();

    await learningApi.quizGrade('course/1', 'round?9', 'act#1', {});

    expect(captured[0]?.url).toContain('/courses/course%2F1/quiz/rounds/round%3F9/grade');
    expect(captured[0]?.body).toEqual({ activity_id: 'act#1', response: {} });
  });

  test('校准读面本波不接：learningApi 无 quizCalibration（u4 票面裁决前的节制）', () => {
    expect('quizCalibration' in learningApi).toBe(false);
  });
});
