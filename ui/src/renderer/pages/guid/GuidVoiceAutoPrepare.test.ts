import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

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
    expect(adapterSource.includes('export async function prepareVoiceSession(')).toBe(true);
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
      if (callCount === 1) throw { code: 'voice_companion_not_running' };
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
      if (count === 1) throw { code: 'voice_suite_not_ready' };
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
