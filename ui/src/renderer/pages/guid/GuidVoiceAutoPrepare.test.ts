import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';
import * as voice from '../../../common/adapter/voiceCompanion';

declare const Bun: any;

const readSource = (url: URL) => readFileSync(url, 'utf8');

function createProductionHandler(overrides: Partial<Record<string, unknown>> = {}) {
  const source = readFileSync(new URL('./GuidPage.tsx', import.meta.url), 'utf8');
  const marker = 'const handleStartVoiceCall = useCallback(async () => {';
  const begin = source.indexOf(marker) + marker.length;
  const end = source.indexOf('\n  }, [isVoiceConnecting,', begin);
  if (begin < marker.length || end < 0) throw new Error('handleStartVoiceCall not found in GuidPage.tsx');
  const body = source.slice(begin, end);
  const fields = [
    'inFlightVoiceCallRef',
    'isVoiceConnecting',
    'voicePrerequisite',
    'setVoicePrerequisite',
    'voiceFailureReason',
    'setVoiceFailureReason',
    'isVoiceActive',
    'voiceSnapshot',
    'navigate',
    'setIsVoiceConnecting',
    'guidInput',
    'isNomiAgent',
    'mainModelRef',
    'agentSelection',
    'homeSkillChipsRef',
    'useComposerDraftStore',
    'GUID_DRAFT_KEY',
    'startVoiceCallIntent',
    'isMountedRef',
    'Message',
    't',
    'prepareVoiceSession',
    'discardPendingVoiceCallIntent',
    'voiceStartNeedsPrepare',
    'localeKey',
  ];
  const compiled = new Bun.Transpiler({ loader: 'ts' }).transformSync(
    `function create(env: any) { const { ${fields.join(', ')} } = env; return async () => { ${body}\n }; }`
  );
  const factory = new Function(compiled + '\nreturn create;')();

  const calls = {
    start: [] as any[],
    prepare: [] as any[],
    navigate: [] as any[],
    info: [] as any[],
    warning: [] as any[],
    error: [] as any[],
    discard: [] as any[],
    connectingStates: [] as boolean[],
  };

  const inFlightRef = { current: false };
  const mountedRef = { current: true };
  let connectingState = false;

  const env: Record<string, any> = {
    inFlightVoiceCallRef: inFlightRef,
    isVoiceConnecting: connectingState,
    voicePrerequisite: 'ready',
    setVoicePrerequisite: (value: string) => { env.voicePrerequisite = value; },
    voiceFailureReason: null,
    setVoiceFailureReason: (value: string | null) => { env.voiceFailureReason = value; },
    isVoiceActive: false,
    voiceSnapshot: null,
    navigate: async (url: string) => {
      calls.navigate.push(url);
    },
    setIsVoiceConnecting: (val: boolean) => {
      connectingState = val;
      env.isVoiceConnecting = val;
      calls.connectingStates.push(val);
    },
    guidInput: { dir: '', input: '', files: [] },
    isNomiAgent: false,
    mainModelRef: undefined,
    agentSelection: {},
    homeSkillChipsRef: { current: [] },
    useComposerDraftStore: { getState: () => ({ setDraft: () => {} }) },
    GUID_DRAFT_KEY: 'probe',
    startVoiceCallIntent: async (target: any, mode: any, locale: any) => {
      calls.start.push({ target, mode, locale });
      if (env.customStart) {
        return await env.customStart(target, mode, locale);
      }
      return { conversation_id: 'conv-123' };
    },
    isMountedRef: mountedRef,
    Message: {
      info: (msg: string) => calls.info.push(msg),
      warning: (msg: string) => calls.warning.push(msg),
      error: (msg: string) => calls.error.push(msg),
    },
    t: (s: string) => s,
    prepareVoiceSession: async (includeLlm: boolean, loc: string, stg: string) => {
      calls.prepare.push({ includeLlm, loc, stg });
      if (env.customPrepare) {
        return await env.customPrepare(includeLlm, loc, stg);
      }
      return { can_start_full_call: true };
    },
    discardPendingVoiceCallIntent: () => { calls.discard.push(1); },
    voiceStartNeedsPrepare: voice.voiceStartNeedsPrepare,
    localeKey: 'zh-CN',
    ...overrides,
  };

  const handler = factory(env);
  return {
    handler,
    calls,
    inFlightRef,
    mountedRef,
    env,
  };
}

describe('GuidPage Voice Auto Prepare (UX-01 & UX-02)', () => {
  test('an obsolete start does not display an error or recheck capabilities', async () => {
    const handler = createProductionHandler({
      customStart: async () => { throw Object.assign(new Error('superseded'), { code: 'voice_call_superseded' }); },
    });
    await handler.handler();
    expect(handler.calls.error).toEqual([]);
    expect(handler.calls.warning).toEqual([]);
    expect(handler.calls.prepare).toEqual([]);
    expect(handler.calls.navigate).toEqual([]);
  });
  test('an obsolete retry after preparation stays silent', async () => {
    let attempts = 0;
    const handler = createProductionHandler({
      customStart: async () => {
        attempts += 1;
        if (attempts === 1) throw Object.assign(new Error('check'), { status: 409, code: 'voice_suite_not_ready', requires_check: true });
        throw Object.assign(new Error('superseded'), { code: 'voice_call_superseded' });
      },
    });
    await handler.handler();
    expect(handler.calls.prepare).toHaveLength(1);
    expect(handler.calls.error).toEqual([]);
    expect(handler.calls.warning).toEqual([]);
    expect(handler.calls.navigate).toEqual([]);
  });
  test('actual adapter preserves the start identity after prepare and a retry 503', async () => {
    const originalFetch = globalThis.fetch;
    const ids: string[] = [];
    let attempts = 0;
    globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      if (url.endsWith('/session/start')) {
        const request = JSON.parse(String(init?.body));
        ids.push(request.request_id);
        attempts += 1;
        if (attempts === 1) return Response.json({ code: 'voice_suite_not_ready', reason_code: 'self_check_required', requires_check: true }, { status: 409 });
        if (attempts === 2) return Response.json({ code: 'daemon_wake_timeout' }, { status: 503 });
        return Response.json({ request_id: request.request_id, status: 'ready', conversation_id: 'voice-test', session_id: 's' });
      }
      if (url.endsWith('/status')) return Response.json({ autostart: true, models_ready: true, running: true });
      if (url.endsWith('/capabilities')) return Response.json(null);
      if (url.endsWith('/prepare')) return Response.json({ can_start_full_call: true });
      throw new Error(`unexpected endpoint ${url}`);
    }) as typeof fetch;
    const handler = createProductionHandler({
      startVoiceCallIntent: voice.startVoiceCallIntent,
      prepareVoiceSession: voice.prepareVoiceSession,
      discardPendingVoiceCallIntent: voice.discardPendingVoiceCallIntent,
    });
    try {
      await handler.handler();
      expect(voice.getPendingVoiceCallIntent()?.request_id).toBe(ids[0]);
      await handler.handler();
      expect(ids).toHaveLength(3);
      expect(new Set(ids).size).toBe(1);
      expect(handler.calls.navigate).toEqual(['/conversation/voice-test']);
    } finally {
      if (voice.getPendingVoiceCallIntent()) {
        await voice.startVoiceCallIntent(voice.getPendingVoiceCallIntent()!.target).catch(() => {});
      }
      globalThis.fetch = originalFetch;
    }
  });

  test('actual adapter does not recheck a known failed capability', async () => {
    const originalFetch = globalThis.fetch;
    const calls: string[] = [];
    globalThis.fetch = (async (input: RequestInfo | URL) => {
      const url = String(input);
      calls.push(url);
      if (url.endsWith('/session/start')) return Response.json({ code: 'voice_suite_not_ready', reason_code: 'asr_inference_failed', requires_check: false }, { status: 409 });
      if (url.endsWith('/status')) return Response.json({ autostart: true, models_ready: true, running: true });
      if (url.endsWith('/capabilities')) return Response.json(null);
      if (url.endsWith('/prepare')) return Response.json({ can_start_full_call: false, reason_code: 'asr_inference_failed' });
      throw new Error(`unexpected endpoint ${url}`);
    }) as typeof fetch;
    const handler = createProductionHandler({
      startVoiceCallIntent: voice.startVoiceCallIntent,
      prepareVoiceSession: voice.prepareVoiceSession,
      discardPendingVoiceCallIntent: voice.discardPendingVoiceCallIntent,
    });
    try {
      await handler.handler();
      expect(calls).toHaveLength(1);
      expect(calls[0].endsWith('/session/start')).toBe(true);
    } finally {
      voice.discardPendingVoiceCallIntent(voice.getPendingVoiceCallIntent()?.request_id);
      globalThis.fetch = originalFetch;
    }
  });

  test('setup double-click shares one prepare and one start', async () => {
    let release!: (value: { can_start_full_call: boolean }) => void;
    const gate = new Promise<{ can_start_full_call: boolean }>((resolve) => { release = resolve; });
    const handler = createProductionHandler({
      voicePrerequisite: 'setup',
      customPrepare: async () => gate,
    });
    const first = handler.handler();
    const second = handler.handler();
    expect(handler.calls.prepare.length).toBe(1);
    release({ can_start_full_call: true });
    await Promise.all([first, second]);
    expect(handler.calls.prepare.length).toBe(1);
    expect(handler.calls.start.length).toBe(1);
  });

  test('entry gate routes uninstalled or unready suites to settings without starting a call', async () => {
    const notReady = createProductionHandler({ voicePrerequisite: 'setup', customPrepare: async () => ({ can_start_full_call: false }) });
    await notReady.handler();
    expect(notReady.calls.start.length).toBe(0);
    expect(notReady.calls.navigate).toEqual(['/settings/voice']);

    const missing = createProductionHandler({
      voicePrerequisite: 'setup',
      customPrepare: async () => { throw Object.assign(new Error('missing'), { code: 'models_not_installed' }); },
    });
    await missing.handler();
    expect(missing.calls.start.length).toBe(0);
    expect(missing.calls.navigate).toEqual(['/settings/voice']);

    const loading = createProductionHandler({ voicePrerequisite: 'loading' });
    await loading.handler();
    expect(loading.calls.start.length).toBe(0);
    expect(loading.calls.prepare.length).toBe(0);
    expect(loading.calls.navigate.length).toBe(0);

    const unavailable = createProductionHandler({ voicePrerequisite: 'unavailable' });
    await unavailable.handler();
    expect(unavailable.calls.start.length).toBe(0);
    expect(unavailable.calls.prepare.length).toBe(0);
    expect(unavailable.calls.navigate).toEqual(['/settings/voice']);

    const failed = createProductionHandler({ voicePrerequisite: 'failed', voiceFailureReason: 'asr_inference_failed' });
    await failed.handler();
    expect(failed.calls.prepare.length).toBe(0);
    expect(failed.calls.start.length).toBe(0);
    expect(failed.calls.warning[0]).toContain('asr_inference_failed');
    expect(failed.calls.navigate).toEqual(['/settings/voice']);
  });

  test('a TTL-expired installed suite auto-prepares then starts exactly once', async () => {
    const handler = createProductionHandler({
      voicePrerequisite: 'setup',
      customPrepare: async () => ({ can_start_full_call: true }),
    });
    await handler.handler();
    expect(handler.calls.prepare.length).toBe(1);
    expect(handler.calls.start.length).toBe(1);
  });

  test('single-click auto-prepare: eliminates two-click prepareRetryRef and warning prompt', () => {
    const source = readSource(new URL('./GuidPage.tsx', import.meta.url));

    // Verify prepareRetryRef is removed
    expect(source.includes('prepareRetryRef')).toBe(false);

    // Verify single-flight connecting state guards against concurrent clicks
    expect(source.includes('if (inFlightVoiceCallRef.current) return;')).toBe(true);
    expect(source.includes('if (isVoiceConnecting) return;')).toBe(true);
    expect(source.includes('setIsVoiceConnecting(true);')).toBe(true);

    // Verify noisy operation fee/prepare hint is removed from entry flow per user request
    expect(source.includes("Message.info(t('voice.entry.prepareHint'))")).toBe(false);

    // Verify auto-prepare is triggered directly on first click when needsPrepare is true
    expect(source.includes("const report = await prepareVoiceSession(true, localeKey, 'preparing');")).toBe(true);

    // Verify mount guard suppresses late navigation if page is unmounted during async prepare
    expect(source.includes('if (!isMountedRef.current) return;')).toBe(true);

    // Verify seamless auto-start and navigation on successful readiness
    expect(source.includes('const retry = await startVoiceCallIntent(target, sessionMode, localeKey);')).toBe(true);
    expect(source.includes('await navigate(`/conversation/${encodeURIComponent(retry.conversation_id)}`);')).toBe(true);
  });

  test('voiceCompanion adapter propagates locale and stage parameters', () => {
    const adapterSource = readSource(new URL('../../../common/adapter/voiceCompanion.ts', import.meta.url));

    // Verify VoiceCallStartRequest contains locale
    expect(adapterSource.includes('locale?: string;')).toBe(true);

    // Verify startVoiceCallIntent accepts and forwards locale
    expect(adapterSource.includes('startVoiceCallIntent(')).toBe(true);
    expect(adapterSource.includes('locale?: string')).toBe(true);

    // Verify prepareVoiceSession forwards locale and stage
    expect(adapterSource.includes('export function prepareVoiceSession(')).toBe(true);
    expect(adapterSource.includes('include_llm: includeLlm, locale, stage')).toBe(true);

    // Verify runVoiceSelfCheck forwards locale and stage
    expect(adapterSource.includes('export async function runVoiceSelfCheck(')).toBe(true);
  });

  test('behavioral: rapid double clicks coalesce into a single execution', async () => {
    const h = createProductionHandler();
    let resolveStart!: (val: any) => void;
    const pendingPromise = new Promise(res => { resolveStart = res; });
    h.env.customStart = () => pendingPromise;

    const first = h.handler();
    const second = h.handler(); // Concurrent double-click in same tick

    resolveStart({ conversation_id: 'conv-double-click' });
    await Promise.all([first, second]);

    expect(h.calls.start.length).toBe(1);
    expect(h.calls.navigate.length).toBe(1);
    expect(h.calls.navigate[0]).toBe('/conversation/conv-double-click');
    expect(h.inFlightRef.current).toBe(false);
  });

  test('behavioral: initial start failure after unmount never triggers prepareVoiceSession or navigation', async () => {
    const h = createProductionHandler();
    let rejectStart!: (err: any) => void;
    const pendingPromise = new Promise((_, rej) => { rejectStart = rej; });
    h.env.customStart = () => pendingPromise;

    const promise = h.handler();
    // User unmounts / leaves page while start is in flight
    h.mountedRef.current = false;
    rejectStart({ code: 'voice_suite_not_ready' });
    await promise;

    expect(h.calls.prepare.length).toBe(0);
    expect(h.calls.navigate.length).toBe(0);
    expect(h.inFlightRef.current).toBe(false);
  });

  test('behavioral: unmount during prepareVoiceSession suppresses retry start and navigation', async () => {
    const h = createProductionHandler();
    let resolvePrepare!: (val: any) => void;
    const preparePromise = new Promise(res => { resolvePrepare = res; });
    let callCount = 0;
    h.env.customStart = async () => {
      callCount++;
      if (callCount === 1) throw { status: 400, code: 'voice_companion_not_running' };
      return { conversation_id: 'conv-retry' };
    };
    h.env.customPrepare = () => preparePromise;

    const promise = h.handler();
    // Allow microtasks to run up to prepareVoiceSession
    await new Promise(r => setTimeout(r, 10));

    expect(h.calls.info.length).toBe(0); // noisy prepareHint suppressed
    // User navigates away while preparation is running
    h.mountedRef.current = false;
    resolvePrepare({ can_start_full_call: true });
    await promise;

    // Retry should NOT have been dispatched
    expect(callCount).toBe(1);
    expect(h.calls.navigate.length).toBe(0);
    expect(h.inFlightRef.current).toBe(false);
  });

  test('behavioral: clean remount after departure executes auto-prepare lifecycle', async () => {
    // Unmounted previous instance
    const h1 = createProductionHandler();
    h1.mountedRef.current = false;

    // Newly mounted instance
    const h2 = createProductionHandler();
    let count = 0;
    h2.env.customStart = async () => {
      count++;
      if (count === 1) throw { status: 409, code: 'voice_suite_not_ready', requires_check: true };
      return { conversation_id: 'conv-remount' };
    };

    await h2.handler();

    expect(count).toBe(2); // Initial fail -> prepare -> retry success
    expect(h2.calls.prepare.length).toBe(1);
    expect(h2.calls.navigate).toEqual(['/conversation/conv-remount']);
    expect(h2.inFlightRef.current).toBe(false);
  });

  test('behavioral: unknown error / failure resets inFlight lock for subsequent attempts', async () => {
    const h = createProductionHandler();
    h.env.customStart = async () => {
      throw new Error('connection_timeout');
    };

    await h.handler();
    expect(h.calls.error.length).toBe(1);
    expect(h.inFlightRef.current).toBe(false);

    // Next click can proceed after failure
    h.env.customStart = async () => ({ conversation_id: 'conv-recovered' });
    await h.handler();
    expect(h.calls.navigate).toEqual(['/conversation/conv-recovered']);
    expect(h.inFlightRef.current).toBe(false);
  });
});
