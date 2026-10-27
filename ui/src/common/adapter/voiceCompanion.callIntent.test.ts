import { afterEach, expect, test } from 'bun:test';
import {
  abandonPendingVoiceCallIntent,
  discardPendingVoiceCallIntent,
  getPendingVoiceCallIntent,
  retryAbandonedVoiceCallIntent,
  startVoiceCallIntent,
  startVoiceConversation,
} from './voiceCompanion';

const originalFetch = globalThis.fetch;

afterEach(async () => {
  globalThis.fetch = originalFetch;
  if (getPendingVoiceCallIntent()) discardPendingVoiceCallIntent();
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
