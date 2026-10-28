import { readFileSync } from 'node:fs';
import { expect, test } from 'bun:test';
import * as voice from '../../../../../common/adapter/voiceCompanion';

declare const Bun: any;

function createHandler(
  start: typeof voice.startVoiceCallIntent = voice.startVoiceCallIntent,
  prepare: typeof voice.prepareVoiceSession = voice.prepareVoiceSession,
) {
  const source = readFileSync(new URL('./NomiSendBox.tsx', import.meta.url), 'utf8');
  const marker = 'const handleStartVoiceCall = useCallback(async () => {';
  const begin = source.indexOf(marker) + marker.length;
  const end = source.indexOf('\n  }, [conversation_id, isVoiceActiveElsewhere', begin);
  if (begin < marker.length || end < 0) throw new Error('voice entry callback not found');
  const fields = [
    'isVoiceActiveElsewhere', 'activeVoiceConversationId', 'Message', 't', 'navigate', 'localeKey',
    'prepareVoiceSession', 'retryAbandonedVoiceCallIntent', 'setIsVoiceConnecting', 'lifecycleGenerationRef',
    'startVoiceCallIntent', 'conversation_id', 'mountedRef', 'discardPendingVoiceCallIntent', 'voiceStartNeedsPrepare',
  ];
  const code = new Bun.Transpiler({ loader: 'ts' }).transformSync(
    `function create(env: any) { const { ${fields.join(', ')} } = env; return async () => { ${source.slice(begin, end)}\n }; }`,
  );
  const factory = new Function(`${code}\nreturn create;`)();
  const warnings: string[] = [];
  const errors: string[] = [];
  const handler = factory({
    isVoiceActiveElsewhere: false, activeVoiceConversationId: null,
    Message: { info: () => {}, warning: (value: string) => warnings.push(value), error: (value: string) => errors.push(value) },
    t: (value: string) => value, navigate: async () => {}, localeKey: 'en-US',
    prepareVoiceSession: prepare, retryAbandonedVoiceCallIntent: voice.retryAbandonedVoiceCallIntent,
    setIsVoiceConnecting: () => {}, lifecycleGenerationRef: { current: 1 },
    startVoiceCallIntent: start, conversation_id: 'voice-c', mountedRef: { current: true },
    discardPendingVoiceCallIntent: voice.discardPendingVoiceCallIntent, voiceStartNeedsPrepare: voice.voiceStartNeedsPrepare,
  });
  return { handler, warnings, errors };
}

test('an obsolete start does not surface an error in the current conversation', async () => {
  const { handler, warnings, errors } = createHandler(async () => {
    throw Object.assign(new Error('superseded'), { code: 'voice_call_superseded' });
  });
  await handler();
  expect(errors).toEqual([]);
  expect(warnings).toEqual([]);
});

test('an obsolete retry after preparation stays silent', async () => {
  let attempts = 0;
  const { handler, warnings, errors } = createHandler(async () => {
    attempts += 1;
    if (attempts === 1) throw Object.assign(new Error('check'), { status: 409, code: 'voice_suite_not_ready', requires_check: true });
    throw Object.assign(new Error('superseded'), { code: 'voice_call_superseded' });
  }, async () => ({ can_start_full_call: true }) as voice.VoiceCapabilityReport);
  await handler();
  expect(attempts).toBe(2);
  expect(errors).toEqual([]);
  expect(warnings).toEqual([]);
});

test('existing conversation retries an unknown start with the original ID', async () => {
  const originalFetch = globalThis.fetch;
  const ids: string[] = [];
  const locales: string[] = [];
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith('/session/start')) {
      const request = JSON.parse(String(init?.body));
      ids.push(request.request_id);
      locales.push(request.locale);
      if (ids.length === 1) return Response.json({ code: 'voice_suite_not_ready', reason_code: 'self_check_required', requires_check: true }, { status: 409 });
      if (ids.length === 2) return Response.json({ code: 'daemon_wake_timeout' }, { status: 503 });
      return Response.json({ request_id: request.request_id, status: 'ready', conversation_id: 'voice-c', session_id: 's' });
    }
    if (url.endsWith('/status')) return Response.json({ autostart: true, running: true, models_ready: true });
    if (url.endsWith('/capabilities')) return Response.json(null);
    if (url.endsWith('/prepare')) return Response.json({ can_start_full_call: true });
    throw new Error(`unexpected endpoint ${url}`);
  }) as typeof fetch;
  const { handler } = createHandler();
  try {
    await handler();
    expect(voice.getPendingVoiceCallIntent()?.request_id).toBe(ids[0]);
    await handler();
    expect(ids).toHaveLength(3);
    expect(new Set(ids).size).toBe(1);
    expect(locales).toEqual(['en-US', 'en-US', 'en-US']);
    expect(voice.getPendingVoiceCallIntent()).toBeNull();
  } finally {
    if (voice.getPendingVoiceCallIntent()) await voice.startVoiceCallIntent({ kind: 'voice_conversation', conversation_id: 'voice-c' }).catch(() => {});
    globalThis.fetch = originalFetch;
  }
});

test('existing conversation does not automatically recheck a failed capability', async () => {
  const originalFetch = globalThis.fetch;
  const calls: string[] = [];
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const url = String(input);
    calls.push(url);
    if (url.endsWith('/session/start')) return Response.json({ code: 'voice_suite_not_ready', reason_code: 'asr_inference_failed', requires_check: false }, { status: 409 });
    throw new Error(`unexpected endpoint ${url}`);
  }) as typeof fetch;
  const { handler, warnings } = createHandler();
  try {
    await handler();
    expect(calls).toHaveLength(1);
    expect(warnings[0]).toContain('asr_inference_failed');
    expect(voice.getPendingVoiceCallIntent()).toBeNull();
  } finally {
    voice.discardPendingVoiceCallIntent(voice.getPendingVoiceCallIntent()?.request_id);
    globalThis.fetch = originalFetch;
  }
});
