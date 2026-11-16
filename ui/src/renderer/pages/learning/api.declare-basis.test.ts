/** learningApi.declareBasis 的请求体（g2）：已会声明的铸名评审用用户所选模型——
 * 选了模型则 provider_id+model 成对随 doors 发出；未选（choice = null）两字段
 * 都不带，后端回落默认档（后端成对校验：只带其一 = 400）。
 *
 * 观察口径：HTTP 边界——逐用例替换 globalThis.fetch 截获请求体并还原
 * （同 common/adapter/httpBridge.test.ts）。 */
import { afterEach, describe, expect, test } from 'bun:test';
import { learningApi } from './api';

const realFetch = globalThis.fetch;

function captureRequestBodies(): unknown[] {
  const bodies: unknown[] = [];
  globalThis.fetch = ((_url: string, init?: RequestInit) => {
    bodies.push(JSON.parse(String(init?.body)));
    return Promise.resolve(
      new Response(JSON.stringify({ success: true, data: {} }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })
    );
  }) as unknown as typeof fetch;
  return bodies;
}

afterEach(() => {
  globalThis.fetch = realFetch;
});

describe('learningApi.declareBasis request body', () => {
  test('carries the chosen provider_id and model alongside the doors', async () => {
    const bodies = captureRequestBodies();

    await learningApi.declareBasis('course-1', ['门口甲', '门口乙'], {
      provider_id: '0190f5fe-7c00-7a00-8000-000000000201',
      model: 'glm-4.6',
    });

    expect(bodies).toEqual([
      {
        doors: ['门口甲', '门口乙'],
        provider_id: '0190f5fe-7c00-7a00-8000-000000000201',
        model: 'glm-4.6',
      },
    ]);
  });

  test('sends neither field without a model choice, leaving the backend default tier', async () => {
    const bodies = captureRequestBodies();

    await learningApi.declareBasis('course-1', ['门口甲'], null);

    expect(bodies).toEqual([{ doors: ['门口甲'] }]);
  });
});
