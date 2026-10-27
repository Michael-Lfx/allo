import { afterEach, expect, test } from 'bun:test';
import {
  abandonPendingVoiceCallIntent,
  getPendingVoiceCallIntent,
  retryAbandonedVoiceCallIntent,
  startVoiceCallIntent,
} from './voiceCompanion';

const originalFetch = globalThis.fetch;

afterEach(() => {
  globalThis.fetch = originalFetch;
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

test('exhausted retry of abandoned request clears dead intent to unblock next call', async () => {
  globalThis.fetch = (async (input: string | URL | Request) => {
    const url = String(input);
    if (url.includes('/session/start')) {
      return new Response(JSON.stringify({ code: 'temporarily_unavailable' }), { status: 503 });
    }
    if (url.includes('/companion/session')) {
      // Backend confirms session is idle or dead, not active
      return new Response(JSON.stringify({ session_id: null, phase: 'idle' }), { status: 200 });
    }
    return new Response('{}', { status: 404 });
  }) as typeof fetch;

  const first = startVoiceCallIntent({ kind: 'new_voice_conversation' }).catch((error) => error);
  await abandonPendingVoiceCallIntent();
  await first;
  await new Promise((resolve) => setTimeout(resolve, 50));
  expect(getPendingVoiceCallIntent()).not.toBeNull();

  // Retry fails to find any active session; must clear intent
  const recovered = await retryAbandonedVoiceCallIntent();
  expect(recovered).toBe(false);
  expect(getPendingVoiceCallIntent()).toBeNull();
});
