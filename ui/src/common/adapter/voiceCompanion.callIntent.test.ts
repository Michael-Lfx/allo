import { afterEach, expect, test } from 'bun:test';
import {
  abandonPendingVoiceCallIntent,
  discardPendingVoiceCallIntent,
  getPendingVoiceCallIntent,
  retryAbandonedVoiceCallIntent,
  startVoiceCallIntent,
  voiceStartNeedsPrepare,
  startVoiceConversation,
  stopVoiceSession,
  subscribeVoiceSession,
  prepareVoiceSession,
  type VoiceSnapshot,
} from './voiceCompanion';

const originalFetch = globalThis.fetch;

test('prepare budget includes the host cold-start and self-check windows', async () => {
  const originalTimeout = AbortSignal.timeout;
  const budgets: number[] = [];
  AbortSignal.timeout = (ms: number) => { budgets.push(ms); return originalTimeout(ms); };
  globalThis.fetch = (async (input: RequestInfo | URL) => String(input).endsWith('/status')
    ? Response.json({ autostart: true, models_ready: true })
    : Response.json({ can_start_full_call: true })) as typeof fetch;
  try {
    await prepareVoiceSession(true, 'en-US', 'preparing');
    expect(Math.max(...budgets)).toBeGreaterThanOrEqual(135_000);
  } finally {
    AbortSignal.timeout = originalTimeout;
    globalThis.fetch = originalFetch;
  }
});

(test as (name: string, run: () => Promise<void>, timeout: number) => void)('a successful preparation taking longer than ten seconds is not aborted', async () => {
  let aborted = false;
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith('/status')) return Response.json({ autostart: true, running: true, models_ready: true });
    if (url.endsWith('/capabilities')) return Response.json(null);
    if (url.endsWith('/prepare')) return new Promise<Response>((resolve, reject) => {
      const timer = setTimeout(() => resolve(Response.json({ can_start_full_call: true })), 10_500);
      init?.signal?.addEventListener('abort', () => {
        aborted = true;
        clearTimeout(timer);
        reject(init.signal?.reason);
      }, { once: true });
    });
    throw new Error(`unexpected endpoint ${url}`);
  }) as typeof fetch;
  try {
    expect((await prepareVoiceSession(true, 'en-US', 'preparing')).can_start_full_call).toBe(true);
    expect(aborted).toBe(false);
  } finally {
    globalThis.fetch = originalFetch;
  }
}, 15_000);

test('a stalled stop is bounded and remains an unknown result', async () => {
  const originalTimeout = AbortSignal.timeout;
  let stopSignal: AbortSignal | undefined;
  AbortSignal.timeout = () => originalTimeout(20);
  globalThis.fetch = (async (_input: RequestInfo | URL, init?: RequestInit) => {
    stopSignal = init?.signal ?? undefined;
    return new Promise<Response>((resolve, reject) => {
      const fallback = setTimeout(() => resolve(Response.json({})), 120);
      stopSignal?.addEventListener('abort', () => { clearTimeout(fallback); reject(stopSignal?.reason); }, { once: true });
    });
  }) as typeof fetch;
  try {
    const result = await stopVoiceSession({ instance_id: 'i', session_id: 's', turn_id: 't' } as VoiceSnapshot)
      .then(() => 'confirmed', () => 'unknown');
    expect(stopSignal).toBeDefined();
    expect(result).toBe('unknown');
  } finally {
    AbortSignal.timeout = originalTimeout;
    globalThis.fetch = originalFetch;
  }
});

test('overlapping preparation shares one check and rejects a different locale', async () => {
  let resolvePrepare!: (value: Response) => void;
  let prepareCalls = 0;
  const pending = new Promise<Response>((resolve) => { resolvePrepare = resolve; });
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith('/status')) return Response.json({ autostart: true, running: true, models_ready: true });
    if (url.endsWith('/capabilities')) return Response.json(null);
    if (url.endsWith('/prepare')) { prepareCalls += 1; return pending; }
    throw new Error(`unexpected endpoint ${url}`);
  }) as typeof fetch;
  try {
    const first = prepareVoiceSession(true, 'en-US', 'preparing');
    const second = prepareVoiceSession(true, 'en-US', 'preparing');
    expect(second).toBe(first);
    expect((await prepareVoiceSession(true, 'zh-CN', 'preparing').catch((error) => error)).code).toBe('self_check_in_progress');
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(prepareCalls).toBe(1);
    resolvePrepare(Response.json({ can_start_full_call: true }));
    expect((await first).can_start_full_call).toBe(true);
    expect((await second).can_start_full_call).toBe(true);
  } finally {
    resolvePrepare?.(Response.json({ can_start_full_call: false }));
    globalThis.fetch = originalFetch;
  }
});

test('a current capability report prevents a duplicate automatic check', async () => {
  let prepareCalls = 0;
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith('/status')) return Response.json({ autostart: true, running: true, models_ready: true });
    if (url.endsWith('/capabilities')) return Response.json({ can_start_full_call: false, reason_code: 'asr_inference_failed' });
    if (url.endsWith('/prepare')) { prepareCalls += 1; return Response.json({ can_start_full_call: true }); }
    throw new Error(`unexpected endpoint ${url}`);
  }) as typeof fetch;
  try {
    const report = await prepareVoiceSession(true, 'en-US', 'preparing');
    expect(report.reason_code).toBe('asr_inference_failed');
    expect(prepareCalls).toBe(0);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test('a full preparation upgrades a cached report whose LLM check was not run', async () => {
  let prepareCalls = 0;
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith('/status')) return Response.json({ autostart: true, running: true, models_ready: true });
    if (url.endsWith('/capabilities')) return Response.json({
      can_start_full_call: false,
      llm: { status: 'unknown', reason_code: 'llm_check_not_run' },
    });
    if (url.endsWith('/prepare')) {
      prepareCalls += 1;
      return Response.json({ can_start_full_call: true, llm: { status: 'ready' } });
    }
    throw new Error(`unexpected endpoint ${url}`);
  }) as typeof fetch;
  try {
    const report = await prepareVoiceSession(true, 'en-US', 'preparing');
    expect(report.can_start_full_call).toBe(true);
    expect(prepareCalls).toBe(1);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test('a timed-out stop retains the owned identity until a later confirmed stop', async () => {
  const originalTimeout = AbortSignal.timeout;
  let stopSucceeds = false;
  let requestId = '';
  let stopAborted = false;
  AbortSignal.timeout = () => originalTimeout(20);
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith('/session/start')) {
      requestId = JSON.parse(String(init?.body)).request_id;
      return Response.json({ request_id: requestId, status: 'starting', conversation_id: 'c', session_id: 's' });
    }
    if (url.endsWith('/session/stop')) {
      if (stopSucceeds) return Response.json({});
      return new Promise<Response>((_resolve, reject) => {
        const fallback = setTimeout(() => reject(new Error('test_stop_fallback')), 200);
        init?.signal?.addEventListener('abort', () => {
          stopAborted = true;
          clearTimeout(fallback);
          reject(init.signal?.reason);
        }, { once: true });
      });
    }
    if (url.endsWith('/session')) return Response.json({
      instance_id: 'i', session_id: 's', turn_id: 't', phase: 'speaking', ended_reason: null,
      call_request_id: requestId, binding: { session_id: 's', conversation_id: 'c' },
    });
    throw new Error(`unexpected endpoint ${url}`);
  }) as typeof fetch;
  try {
    await startVoiceCallIntent({ kind: 'new_voice_conversation' });
    await abandonPendingVoiceCallIntent();
    expect(stopAborted).toBe(true);
    expect(getPendingVoiceCallIntent()?.request_id).toBe(requestId);
    stopSucceeds = true;
    expect(await retryAbandonedVoiceCallIntent()).toBe(true);
    expect(getPendingVoiceCallIntent()).toBeNull();
  } finally {
    AbortSignal.timeout = originalTimeout;
    globalThis.fetch = originalFetch;
  }
});

test('a later preflight rejection cannot discard an earlier unknown start', async () => {
  let attempt = 0;
  const ids: string[] = [];
  globalThis.fetch = (async (_input: RequestInfo | URL, init?: RequestInit) => {
    const request = JSON.parse(String(init?.body));
    ids.push(request.request_id);
    attempt += 1;
    if (attempt === 1) return Response.json({ code: 'daemon_wake_timeout' }, { status: 503 });
    if (attempt === 2) return Response.json({ code: 'voice_suite_not_ready', reason_code: 'self_check_required', requires_check: true }, { status: 409 });
    return Response.json({ request_id: request.request_id, status: 'ready', conversation_id: 'c', session_id: 's' });
  }) as typeof fetch;
  const target = { kind: 'new_voice_conversation' } as const;
  try {
    await startVoiceCallIntent(target).catch(() => {});
    const requestId = getPendingVoiceCallIntent()!.request_id;
    const error = await startVoiceCallIntent(target).catch((value) => value);
    expect(error.start_outcome_unknown).toBe(true);
    discardPendingVoiceCallIntent(getPendingVoiceCallIntent()?.request_id);
    expect(getPendingVoiceCallIntent()?.request_id).toBe(requestId);
    await startVoiceCallIntent(target);
    expect(new Set(ids).size).toBe(1);
  } finally {
    if (getPendingVoiceCallIntent()) await startVoiceCallIntent(target).catch(() => {});
    globalThis.fetch = originalFetch;
  }
});

test('a response for a different request cannot settle the owned identity', async () => {
  let mismatched = true;
  globalThis.fetch = (async (_input: RequestInfo | URL, init?: RequestInit) => {
    const request = JSON.parse(String(init?.body));
    return Response.json({ request_id: mismatched ? 'other-request' : request.request_id, status: 'ready', conversation_id: 'c', session_id: 's' });
  }) as typeof fetch;
  const target = { kind: 'new_voice_conversation' } as const;
  try {
    const error = await startVoiceCallIntent(target).catch((value) => value);
    expect(error.code).toBe('voice_call_identity_mismatch');
    const originalId = getPendingVoiceCallIntent()!.request_id;
    discardPendingVoiceCallIntent(getPendingVoiceCallIntent()?.request_id);
    expect(getPendingVoiceCallIntent()?.request_id).toBe(originalId);
    mismatched = false;
    await startVoiceCallIntent(target);
    expect(getPendingVoiceCallIntent()).toBeNull();
  } finally {
    mismatched = false;
    if (getPendingVoiceCallIntent()) await startVoiceCallIntent(target).catch(() => {});
    globalThis.fetch = originalFetch;
  }
});

test('a confirmed terminal rejection is replayed with the same ID before a later fresh start', async () => {
  const ids: string[] = [];
  globalThis.fetch = (async (_input: RequestInfo | URL, init?: RequestInit) => {
    const request = JSON.parse(String(init?.body));
    ids.push(request.request_id);
    if (ids.length <= 2) return Response.json({ code: 'daemon_wake_rejected', terminal: true, request_id: request.request_id }, { status: 503 });
    return Response.json({ request_id: request.request_id, status: 'ready', conversation_id: 'c', session_id: 's' });
  }) as typeof fetch;
  const target = { kind: 'new_voice_conversation' } as const;
  try {
    const error = await startVoiceCallIntent(target).catch((value) => value);
    expect(error.code).toBe('daemon_wake_rejected');
    expect(ids).toHaveLength(2);
    expect(ids[1]).toBe(ids[0]);
    expect(getPendingVoiceCallIntent()).toBeNull();
    await startVoiceCallIntent(target);
    expect(ids[2]).not.toBe(ids[0]);
  } finally {
    if (getPendingVoiceCallIntent()) await startVoiceCallIntent(target).catch(() => {});
    globalThis.fetch = originalFetch;
  }
});

test('an unconfirmed terminal receipt never clears the original request', async () => {
  const ids: string[] = [];
  let scenario: 'wrong-id' | 'verification-fails' | 'success' = 'wrong-id';
  globalThis.fetch = (async (_input: RequestInfo | URL, init?: RequestInit) => {
    const request = JSON.parse(String(init?.body));
    ids.push(request.request_id);
    if (scenario === 'wrong-id') return Response.json({ code: 'daemon_wake_rejected', terminal: true, request_id: 'another-request' }, { status: 503 });
    if (scenario === 'verification-fails') {
      if (ids.length === 2) return Response.json({ code: 'daemon_wake_rejected', terminal: true, request_id: request.request_id }, { status: 503 });
      return Response.json({ code: 'daemon_wake_timeout' }, { status: 503 });
    }
    return Response.json({ request_id: request.request_id, status: 'ready', conversation_id: 'c', session_id: 's' });
  }) as typeof fetch;
  const target = { kind: 'new_voice_conversation' } as const;
  try {
    await startVoiceCallIntent(target).catch(() => {});
    const requestId = getPendingVoiceCallIntent()!.request_id;
    expect(ids).toHaveLength(1);
    scenario = 'verification-fails';
    await startVoiceCallIntent(target).catch(() => {});
    expect(ids).toHaveLength(3);
    expect(getPendingVoiceCallIntent()?.request_id).toBe(requestId);
    scenario = 'success';
    await startVoiceCallIntent(target);
    expect(new Set(ids).size).toBe(1);
  } finally {
    scenario = 'success';
    if (getPendingVoiceCallIntent()) await startVoiceCallIntent(target).catch(() => {});
    globalThis.fetch = originalFetch;
  }
});

test('a successful same-ID replay after a terminal-looking 503 returns the actual start result', async () => {
  const ids: string[] = [];
  globalThis.fetch = (async (_input: RequestInfo | URL, init?: RequestInit) => {
    const request = JSON.parse(String(init?.body));
    ids.push(request.request_id);
    if (ids.length === 1) return Response.json({ code: 'daemon_wake_rejected', terminal: true, request_id: request.request_id }, { status: 503 });
    return Response.json({ request_id: request.request_id, status: 'ready', conversation_id: 'c', session_id: 's' });
  }) as typeof fetch;
  const target = { kind: 'new_voice_conversation' } as const;
  try {
    const result = await startVoiceCallIntent(target);
    expect(result.status).toBe('ready');
    expect(ids).toHaveLength(2);
    expect(ids[1]).toBe(ids[0]);
    expect(getPendingVoiceCallIntent()).toBeNull();
  } finally {
    if (getPendingVoiceCallIntent()) await startVoiceCallIntent(target).catch(() => {});
    globalThis.fetch = originalFetch;
  }
});

test('cleanup for a rejected A cannot discard a later rejected B', async () => {
  globalThis.fetch = (async () => Response.json({
    code: 'voice_suite_not_ready', reason_code: 'self_check_required', requires_check: true,
  }, { status: 409 })) as typeof fetch;
  try {
    await startVoiceCallIntent({ kind: 'voice_conversation', conversation_id: 'A' }).catch(() => {});
    const requestA = getPendingVoiceCallIntent()!.request_id;
    await abandonPendingVoiceCallIntent();
    expect(getPendingVoiceCallIntent()).toBeNull();
    await startVoiceCallIntent({ kind: 'voice_conversation', conversation_id: 'B' }).catch(() => {});
    const requestB = getPendingVoiceCallIntent()!.request_id;
    expect(requestB).not.toBe(requestA);
    discardPendingVoiceCallIntent(requestA);
    expect(getPendingVoiceCallIntent()?.request_id).toBe(requestB);
    discardPendingVoiceCallIntent(requestB);
    expect(getPendingVoiceCallIntent()).toBeNull();
  } finally {
    if (getPendingVoiceCallIntent()) discardPendingVoiceCallIntent(getPendingVoiceCallIntent()?.request_id);
    globalThis.fetch = originalFetch;
  }
});

test('a late start success from A cannot supersede a newer pending B', async () => {
  let resolveA!: (value: Response) => void;
  let resolveB!: (value: Response) => void;
  const firstResponse = new Promise<Response>((resolve) => { resolveA = resolve; });
  const secondResponse = new Promise<Response>((resolve) => { resolveB = resolve; });
  let starts = 0;
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    if (String(input).endsWith('/session/stop')) return Response.json({});
    starts += 1;
    return starts === 1 ? firstResponse : secondResponse;
  }) as typeof fetch;
  try {
    const first = startVoiceCallIntent({ kind: 'voice_conversation', conversation_id: 'A' }).catch((error) => error);
    const requestA = getPendingVoiceCallIntent()!.request_id;
    await stopVoiceSession({ instance_id: 'i', session_id: 's-A', turn_id: 't', call_request_id: requestA } as VoiceSnapshot);
    const second = startVoiceCallIntent({ kind: 'voice_conversation', conversation_id: 'B' }).catch((error) => error);
    const requestB = getPendingVoiceCallIntent()!.request_id;
    resolveA(Response.json({ request_id: requestA, status: 'ready', conversation_id: 'A', session_id: 's-A' }));
    expect((await first).code).toBe('voice_call_superseded');
    expect(getPendingVoiceCallIntent()?.request_id).toBe(requestB);
    resolveB(Response.json({ request_id: requestB, status: 'ready', conversation_id: 'B', session_id: 's-B' }));
    expect((await second).conversation_id).toBe('B');
    expect(getPendingVoiceCallIntent()).toBeNull();
  } finally {
    resolveA?.(Response.json({}));
    resolveB?.(Response.json({}));
    globalThis.fetch = originalFetch;
  }
});

test('a late rejected start from A cannot trigger preparation while B owns the request', async () => {
  let resolveA!: (value: Response) => void;
  let resolveB!: (value: Response) => void;
  const responseA = new Promise<Response>((resolve) => { resolveA = resolve; });
  const responseB = new Promise<Response>((resolve) => { resolveB = resolve; });
  let starts = 0;
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    if (String(input).endsWith('/session/stop')) return Response.json({});
    starts += 1;
    return starts === 1 ? responseA : responseB;
  }) as typeof fetch;
  try {
    const first = startVoiceCallIntent({ kind: 'voice_conversation', conversation_id: 'A' }).catch((error) => error);
    const requestA = getPendingVoiceCallIntent()!.request_id;
    await stopVoiceSession({ instance_id: 'i', session_id: 's-A', turn_id: 't', call_request_id: requestA } as VoiceSnapshot);
    const second = startVoiceCallIntent({ kind: 'voice_conversation', conversation_id: 'B' });
    const requestB = getPendingVoiceCallIntent()!.request_id;
    resolveA(Response.json({ code: 'voice_suite_not_ready', reason_code: 'self_check_required', requires_check: true }, { status: 409 }));
    const failure = await first;
    expect(failure.code).toBe('voice_call_superseded');
    expect(voiceStartNeedsPrepare(failure)).toBe(false);
    expect(getPendingVoiceCallIntent()?.request_id).toBe(requestB);
    resolveB(Response.json({ request_id: requestB, status: 'ready', conversation_id: 'B', session_id: 's-B' }));
    expect((await second).conversation_id).toBe('B');
  } finally {
    resolveA?.(Response.json({}));
    resolveB?.(Response.json({}));
    globalThis.fetch = originalFetch;
  }
});

test('a late start success cannot reopen its already stopped call', async () => {
  let resolveStart!: (value: Response) => void;
  const response = new Promise<Response>((resolve) => { resolveStart = resolve; });
  globalThis.fetch = (async (input: RequestInfo | URL) =>
    String(input).endsWith('/session/stop') ? Response.json({}) : response) as typeof fetch;
  try {
    const start = startVoiceCallIntent({ kind: 'voice_conversation', conversation_id: 'A' }).catch((error) => error);
    const requestId = getPendingVoiceCallIntent()!.request_id;
    await stopVoiceSession({ instance_id: 'i', session_id: 's-A', turn_id: 't', call_request_id: requestId } as VoiceSnapshot);
    resolveStart(Response.json({ request_id: requestId, status: 'ready', conversation_id: 'A', session_id: 's-A' }));
    expect((await start).code).toBe('voice_call_superseded');
    expect(getPendingVoiceCallIntent()).toBeNull();
  } finally {
    resolveStart?.(Response.json({}));
    globalThis.fetch = originalFetch;
  }
});

test('late A cannot reopen after a bound event, stop, and a completed B', async () => {
  let resolveA!: (value: Response) => void;
  const responseA = new Promise<Response>((resolve) => { resolveA = resolve; });
  let starts = 0;
  let snapshotA!: VoiceSnapshot;
  let unsubscribe = () => {};
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith('/events')) return new Response(new ReadableStream({ start() {} }));
    if (url.endsWith('/session/stop')) return Response.json({});
    if (url.endsWith('/session')) return Response.json(snapshotA);
    const request = JSON.parse(String(init?.body));
    starts += 1;
    if (starts === 1) return responseA;
    return Response.json({ request_id: request.request_id, status: 'ready', conversation_id: 'B', session_id: 's-B' });
  }) as typeof fetch;
  try {
    const first = startVoiceCallIntent({ kind: 'voice_conversation', conversation_id: 'A' }).catch((error) => error);
    const requestA = getPendingVoiceCallIntent()!.request_id;
    snapshotA = {
      instance_id: 'late-a-host', seq: 1, session_id: 's-A', turn_id: 't-A', phase: 'listening', call_request_id: requestA,
      binding: { session_id: 's-A', binding_id: 'bind-A', conversation_id: 'A', workspace: 'D:/example' },
    } as VoiceSnapshot;
    unsubscribe = await subscribeVoiceSession(() => {}, false);
    expect(getPendingVoiceCallIntent()).toBeNull();
    await stopVoiceSession(snapshotA);
    const second = await startVoiceCallIntent({ kind: 'voice_conversation', conversation_id: 'B' });
    expect(second.conversation_id).toBe('B');
    expect(getPendingVoiceCallIntent()).toBeNull();
    resolveA(Response.json({ request_id: requestA, status: 'ready', conversation_id: 'A', session_id: 's-A' }));
    expect((await first).code).toBe('voice_call_superseded');
  } finally {
    resolveA?.(Response.json({}));
    unsubscribe();
    globalThis.fetch = originalFetch;
  }
});

test('late A cannot reopen after a bound event and stop even without B', async () => {
  let resolveStart!: (value: Response) => void;
  const response = new Promise<Response>((resolve) => { resolveStart = resolve; });
  let snapshot!: VoiceSnapshot;
  let unsubscribe = () => {};
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith('/events')) return new Response(new ReadableStream({ start() {} }));
    if (url.endsWith('/session/stop')) return Response.json({});
    if (url.endsWith('/session')) return Response.json(snapshot);
    return response;
  }) as typeof fetch;
  try {
    const start = startVoiceCallIntent({ kind: 'voice_conversation', conversation_id: 'A' }).catch((error) => error);
    const requestId = getPendingVoiceCallIntent()!.request_id;
    snapshot = {
      instance_id: 'stopped-a-host', seq: 1, session_id: 's-A', turn_id: 't-A', phase: 'listening', call_request_id: requestId,
      binding: { session_id: 's-A', binding_id: 'bind-A', conversation_id: 'A', workspace: 'D:/example' },
    } as VoiceSnapshot;
    unsubscribe = await subscribeVoiceSession(() => {}, false);
    expect(getPendingVoiceCallIntent()).toBeNull();
    await stopVoiceSession(snapshot);
    resolveStart(Response.json({ request_id: requestId, status: 'ready', conversation_id: 'A', session_id: 's-A' }));
    expect((await start).code).toBe('voice_call_superseded');
  } finally {
    resolveStart?.(Response.json({}));
    unsubscribe();
    globalThis.fetch = originalFetch;
  }
});

afterEach(async () => {
  globalThis.fetch = originalFetch;
  if (getPendingVoiceCallIntent()) discardPendingVoiceCallIntent(getPendingVoiceCallIntent()?.request_id);
});

test('abandoned start queries the owned request and stops the active call', async () => {
  let resolveStart!: (value: Response) => void;
  let stopCalls = 0;
  const startResponse = new Promise<Response>((resolve) => { resolveStart = resolve; });
  globalThis.fetch = (async (input: string | URL | Request) => {
    const url = String(input);
    if (url.includes('/session/start')) return startResponse;
    if (url.includes('/session/stop')) {
      stopCalls += 1;
      return new Response('{}', { status: 200 });
    }
    if (url.includes('/companion/session')) {
      return new Response(JSON.stringify({
        instance_id: 'i', seq: 1, session_id: 's', turn_id: 't', phase: 'speaking',
        ended_reason: null, call_request_id: requestId, binding: { session_id: 's', conversation_id: 'c' },
      }), { status: 200 });
    }
    return new Response('{}', { status: 404 });
  }) as typeof fetch;

  const starting = startVoiceCallIntent({ kind: 'new_voice_conversation' });
  // The owned request id is assigned synchronously before the fetch resolves.
  const requestId = getPendingVoiceCallIntent()!.request_id;
  await abandonPendingVoiceCallIntent();
  resolveStart(new Response(JSON.stringify({
    request_id: requestId, status: 'starting', conversation_id: 'c', session_id: 's',
  }), { status: 200 }));
  await starting;
  await new Promise((resolve) => setTimeout(resolve, 250));
  expect(stopCalls).toBe(1);
  expect(getPendingVoiceCallIntent()).toBeNull();
});

test('unknown lookup failures retain the original request id and never allocate a new one', async () => {
  let startCalls = 0;
  let lookupMode: 'fail' | 'active' = 'fail';
  globalThis.fetch = (async (input: string | URL | Request) => {
    const url = String(input);
    if (url.includes('/session/start')) {
      startCalls += 1;
      return new Response(JSON.stringify({ code: 'temporarily_unavailable' }), { status: 503 });
    }
    if (url.includes('/session/stop')) return new Response('{}', { status: 200 });
    if (url.includes('/companion/session')) {
      if (lookupMode === 'fail') return new Response('{}', { status: 503 });
      const requestId = getPendingVoiceCallIntent()!.request_id;
      return new Response(JSON.stringify({
        instance_id: 'i', seq: 1, session_id: 's', turn_id: 't', phase: 'speaking',
        ended_reason: null, call_request_id: requestId, binding: { session_id: 's', conversation_id: 'c' },
      }), { status: 200 });
    }
    return new Response('{}', { status: 404 });
  }) as typeof fetch;

  const first = startVoiceCallIntent({ kind: 'new_voice_conversation' }).catch((error) => error);
  const originalId = getPendingVoiceCallIntent()!.request_id;
  await abandonPendingVoiceCallIntent();
  await first;
  await new Promise((resolve) => setTimeout(resolve, 100));
  expect(getPendingVoiceCallIntent()?.request_id).toBe(originalId);
  const next = await startVoiceCallIntent({ kind: 'new_voice_conversation' }).catch((error) => error);
  expect(next.code).toBe('voice_call_cancel_unknown');
  expect(startCalls).toBe(1);

  lookupMode = 'active';
  expect(await retryAbandonedVoiceCallIntent()).toBe(true);
  expect(getPendingVoiceCallIntent()).toBeNull();
});

test('an idle snapshot without the owned request id stays unknown until a terminal receipt', async () => {
  let terminal = false;
  globalThis.fetch = (async (input: string | URL | Request) => {
    const url = String(input);
    if (url.includes('/session/start')) {
      return new Response(JSON.stringify({ code: 'temporarily_unavailable' }), { status: 503 });
    }
    if (url.includes('/companion/session')) {
      if (!terminal) return new Response(JSON.stringify({ session_id: null, phase: 'idle' }), { status: 200 });
      return new Response(JSON.stringify({
        instance_id: 'i', seq: 1, session_id: 's', turn_id: 't', phase: 'idle',
        ended_reason: 'user_stop', call_request_id: getPendingVoiceCallIntent()!.request_id,
        binding: null,
      }), { status: 200 });
    }
    return new Response('{}', { status: 404 });
  }) as typeof fetch;

  const first = startVoiceCallIntent({ kind: 'new_voice_conversation' }).catch((error) => error);
  await abandonPendingVoiceCallIntent();
  await first;
  expect(getPendingVoiceCallIntent()).not.toBeNull();

  const unknown = await retryAbandonedVoiceCallIntent();
  expect(unknown).toBe(false);
  expect(getPendingVoiceCallIntent()).not.toBeNull();

  terminal = true;
  const recovered = await retryAbandonedVoiceCallIntent();
  expect(recovered).toBe(false);
  expect(getPendingVoiceCallIntent()).toBeNull();
});

test('a matching request id without a session or with an idle session is not terminal', async () => {
  let stage: 'no-session' | 'idle-session' | 'ended' = 'no-session';
  globalThis.fetch = (async (input: string | URL | Request) => {
    const url = String(input);
    if (url.includes('/session/start')) {
      return new Response(JSON.stringify({ code: 'temporarily_unavailable' }), { status: 503 });
    }
    if (url.includes('/companion/session')) {
      const requestId = getPendingVoiceCallIntent()!.request_id;
      if (stage === 'no-session') {
        return new Response(JSON.stringify({
          instance_id: 'i', seq: 1, session_id: null, turn_id: null, phase: 'idle',
          ended_reason: null, call_request_id: requestId, binding: null,
        }), { status: 200 });
      }
      if (stage === 'idle-session') {
        return new Response(JSON.stringify({
          instance_id: 'i', seq: 2, session_id: 's', turn_id: 't', phase: 'idle',
          ended_reason: null, call_request_id: requestId, binding: { session_id: 's', conversation_id: 'c' },
        }), { status: 200 });
      }
      return new Response(JSON.stringify({
        instance_id: 'i', seq: 3, session_id: 's', turn_id: 't', phase: 'idle',
        ended_reason: 'user_stop', call_request_id: requestId, binding: null,
      }), { status: 200 });
    }
    return new Response('{}', { status: 404 });
  }) as typeof fetch;

  const first = startVoiceCallIntent({ kind: 'new_voice_conversation' }).catch((error) => error);
  const requestId = getPendingVoiceCallIntent()!.request_id;
  await abandonPendingVoiceCallIntent();
  await first;
  expect(await retryAbandonedVoiceCallIntent()).toBe(false);
  expect(getPendingVoiceCallIntent()?.request_id).toBe(requestId);

  stage = 'idle-session';
  expect(await retryAbandonedVoiceCallIntent()).toBe(false);
  expect(getPendingVoiceCallIntent()?.request_id).toBe(requestId);

  stage = 'ended';
  expect(await retryAbandonedVoiceCallIntent()).toBe(false);
  expect(getPendingVoiceCallIntent()).toBeNull();
});

test('abandoned successful start keeps its request id when the stop outcome is unknown', async () => {
  let resolveStart!: (value: Response) => void;
  const startResponse = new Promise<Response>((resolve) => { resolveStart = resolve; });
  let lookups = 0;
  globalThis.fetch = (async (input: string | URL | Request) => {
    const url = String(input);
    if (url.includes('/session/start')) return startResponse;
    if (url.includes('/session/stop')) return new Response('{}', { status: 200 });
    if (url.includes('/companion/session')) { lookups += 1; return new Response('{}', { status: 503 }); }
    return new Response('{}', { status: 404 });
  }) as typeof fetch;

  const starting = startVoiceCallIntent({ kind: 'new_voice_conversation' });
  const requestId = getPendingVoiceCallIntent()!.request_id;
  await abandonPendingVoiceCallIntent();
  resolveStart(new Response(JSON.stringify({
    request_id: requestId, status: 'starting', conversation_id: 'c', session_id: 's',
  }), { status: 200 }));
  await starting.catch(() => {});

  expect(lookups).toBeGreaterThanOrEqual(5);
  expect(getPendingVoiceCallIntent()?.request_id).toBe(requestId);
  expect(await retryAbandonedVoiceCallIntent()).toBe(false);
  expect(getPendingVoiceCallIntent()?.request_id).toBe(requestId);

  globalThis.fetch = (async (input: string | URL | Request) => {
    if (String(input).includes('/companion/session')) {
      return new Response(JSON.stringify({
        instance_id: 'i', seq: 2, session_id: 's', turn_id: 't', phase: 'idle',
        ended_reason: 'user_stop', call_request_id: requestId, binding: null,
      }), { status: 200 });
    }
    return new Response('{}', { status: 404 });
  }) as typeof fetch;
  expect(await retryAbandonedVoiceCallIntent()).toBe(false);
  expect(getPendingVoiceCallIntent()).toBeNull();
});

test('a failed stop is never treated as an authoritative cancellation', async () => {
  let stopStatus = 500;
  globalThis.fetch = (async (input: string | URL | Request) => {
    const url = String(input);
    if (url.includes('/session/start')) {
      return new Response(JSON.stringify({
        request_id: getPendingVoiceCallIntent()!.request_id, status: 'starting', conversation_id: 'c', session_id: 's',
      }), { status: 200 });
    }
    if (url.includes('/session/stop')) return new Response('{}', { status: stopStatus });
    if (url.includes('/companion/session')) {
      return new Response(JSON.stringify({
        instance_id: 'i', seq: 1, session_id: 's', turn_id: 't', phase: 'speaking',
        ended_reason: null, call_request_id: getPendingVoiceCallIntent()!.request_id,
        binding: { session_id: 's', conversation_id: 'c' },
      }), { status: 200 });
    }
    return new Response('{}', { status: 404 });
  }) as typeof fetch;

  const started = await startVoiceCallIntent({ kind: 'new_voice_conversation' });
  expect(started.status).toBe('starting');
  const requestId = getPendingVoiceCallIntent()!.request_id;
  await abandonPendingVoiceCallIntent();
  expect(getPendingVoiceCallIntent()?.request_id).toBe(requestId);
  expect(await retryAbandonedVoiceCallIntent()).toBe(false);
  expect(getPendingVoiceCallIntent()?.request_id).toBe(requestId);

  stopStatus = 200;
  expect(await retryAbandonedVoiceCallIntent()).toBe(true);
  expect(getPendingVoiceCallIntent()).toBeNull();
});

test('404, 409 and a different active request do not prove the owned request is dead', async () => {
  let lookup: '404' | '409' | 'other' = '404';
  globalThis.fetch = (async (input: string | URL | Request) => {
    const url = String(input);
    if (url.includes('/session/start')) {
      return new Response(JSON.stringify({ code: 'temporarily_unavailable' }), { status: 503 });
    }
    if (url.includes('/companion/session')) {
      if (lookup === '404') return new Response('{}', { status: 404 });
      if (lookup === '409') return new Response('{}', { status: 409 });
      return new Response(JSON.stringify({
        instance_id: 'i', seq: 1, session_id: 'other', turn_id: 't', phase: 'speaking',
        ended_reason: null, call_request_id: 'another-request', binding: { session_id: 'other', conversation_id: 'other' },
      }), { status: 200 });
    }
    return new Response('{}', { status: 404 });
  }) as typeof fetch;

  const first = startVoiceCallIntent({ kind: 'new_voice_conversation' }).catch((error) => error);
  const requestId = getPendingVoiceCallIntent()!.request_id;
  await abandonPendingVoiceCallIntent();
  await first;

  for (const mode of ['404', '409', 'other'] as const) {
    lookup = mode;
    expect(await retryAbandonedVoiceCallIntent()).toBe(false);
    expect(getPendingVoiceCallIntent()?.request_id).toBe(requestId);
  }

  globalThis.fetch = (async (input: string | URL | Request) => {
    if (String(input).includes('/companion/session')) {
      return new Response(JSON.stringify({
        instance_id: 'i', seq: 2, session_id: 's', turn_id: 't', phase: 'idle',
        ended_reason: 'user_stop', call_request_id: requestId, binding: null,
      }), { status: 200 });
    }
    return new Response('{}', { status: 404 });
  }) as typeof fetch;
  expect(await retryAbandonedVoiceCallIntent()).toBe(false);
  expect(getPendingVoiceCallIntent()).toBeNull();
});

test('an unknown status never reaches prepare or autostart', async () => {
  const originalWindow = (globalThis as any).window;
  (globalThis as any).window = { location: { hash: '' } };
  let prepareCalls = 0;
  globalThis.fetch = (async (input: string | URL | Request) => {
    const url = String(input);
    if (url.includes('/session/start')) {
      return new Response(JSON.stringify({ code: 'voice_companion_not_running' }), { status: 400 });
    }
    if (url.includes('/companion/status')) return new Response('{}', { status: 503 });
    if (url.includes('/companion/prepare')) {
      prepareCalls += 1;
      return new Response(JSON.stringify({ code: 'prepare_must_not_run' }), { status: 500 });
    }
    return new Response('{}', { status: 404 });
  }) as typeof fetch;
  try {
    const error = await startVoiceConversation('zh-CN').catch((value) => value);
    expect(error.code).toBe('voice_status_unavailable');
    expect(prepareCalls).toBe(0);
    expect(getPendingVoiceCallIntent()).toBeNull();
  } finally {
    (globalThis as any).window = originalWindow;
  }
});

test('a disabled user never reaches prepare or autostart', async () => {
  const originalWindow = (globalThis as any).window;
  (globalThis as any).window = { location: { hash: '' } };
  let prepareCalls = 0;
  globalThis.fetch = (async (input: string | URL | Request) => {
    const url = String(input);
    if (url.includes('/session/start')) {
      return new Response(JSON.stringify({ code: 'voice_companion_not_running' }), { status: 400 });
    }
    if (url.includes('/companion/status')) {
      return new Response(JSON.stringify({ running: false, autostart: false, models_ready: true }), { status: 200 });
    }
    if (url.includes('/companion/prepare')) {
      prepareCalls += 1;
      return new Response(JSON.stringify({ code: 'prepare_must_not_run' }), { status: 500 });
    }
    return new Response('{}', { status: 404 });
  }) as typeof fetch;
  try {
    const error = await startVoiceConversation('en-US').catch((value) => value);
    expect(error.code).toBe('voice_disabled');
    expect(prepareCalls).toBe(0);
    expect(getPendingVoiceCallIntent()).toBeNull();
  } finally {
    (globalThis as any).window = originalWindow;
  }
});
