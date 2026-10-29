import * as bunTest from 'bun:test';
const { describe, expect, test } = bunTest;
const afterAll = (bunTest as unknown as { afterAll: (callback: () => void | Promise<void>) => void }).afterAll;
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { renderToStaticMarkup } from 'react-dom/server';
// @ts-expect-error jsdom has no local declaration in this workspace
import { JSDOM } from 'jsdom';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import { MemoryRouter, Route, Routes, useLocation, useNavigate } from 'react-router-dom';
import { getPendingVoiceCallIntent } from '@common/adapter/voiceCompanion';
import { CloudAuthContext, type CloudAuthContextValue } from '@renderer/hooks/context/CloudAuthContext';
import { ThemeContext, type ThemeContextValue } from '@renderer/hooks/context/ThemeContext';
import { ConversationHistoryProvider } from '@renderer/hooks/context/ConversationHistoryContext';
import ComposerSubmitCluster from './ComposerSubmitCluster';
import GuidPage from '@renderer/pages/guid/GuidPage';
import zh from '@renderer/services/i18n/locales/zh-CN/voice.json';
import en from '@renderer/services/i18n/locales/en-US/voice.json';

let domMounted = false;
let root: Root | null = null;
let container: HTMLElement;
let globalInstanceSeq = 1000;
let testDom: JSDOM | null = null;
const originalGlobals = new Map<string, PropertyDescriptor | undefined>();

function ensureDom() {
  if (domMounted) return;
  domMounted = true;
  const globalsToRestore = [
    'Element', 'HTMLElement', 'Node', 'Event', 'CustomEvent', 'MouseEvent', 'KeyboardEvent', 'DocumentFragment',
    'Blob', 'File', 'window', 'document', 'navigator', 'IS_REACT_ACT_ENVIRONMENT',
    'getComputedStyle', 'requestAnimationFrame', 'cancelAnimationFrame', 'ResizeObserver', 'IntersectionObserver', 'matchMedia', 'WebSocket'
  ] as const;
  for (const name of globalsToRestore) originalGlobals.set(name, Object.getOwnPropertyDescriptor(globalThis, name));

  const domInstance = new JSDOM('<!doctype html><html><body><div id="root"></div></body></html>', { url: 'http://localhost/' });
  testDom = domInstance;
  const win = domInstance.window as unknown as Window & typeof globalThis;
  (win as unknown as { isSecureContext: boolean }).isSecureContext = true;
  for (const name of ['Element','HTMLElement','Node','Event','CustomEvent','MouseEvent','KeyboardEvent','DocumentFragment','Blob','File'] as const) {
    Object.defineProperty(globalThis, name, { configurable: true, value: (win as unknown as Record<string, unknown>)[name] });
  }
  Object.defineProperty(globalThis, 'window', { configurable: true, value: win });
  Object.defineProperty(globalThis, 'document', { configurable: true, value: win.document });
  Object.defineProperty(globalThis, 'navigator', { configurable: true, value: win.navigator });
  Object.defineProperty(globalThis, 'IS_REACT_ACT_ENVIRONMENT', { configurable: true, value: true });
  const raf = (cb: FrameRequestCallback) => setTimeout(() => cb(Date.now()), 16) as unknown as number;
  const caf = (id: number) => clearTimeout(id);
  Object.defineProperty(win, 'requestAnimationFrame', { configurable: true, value: raf });
  Object.defineProperty(win, 'cancelAnimationFrame', { configurable: true, value: caf });
  Object.defineProperty(globalThis, 'getComputedStyle', { configurable: true, value: win.getComputedStyle.bind(win) });
  Object.defineProperty(globalThis, 'requestAnimationFrame', { configurable: true, value: raf });
  Object.defineProperty(globalThis, 'cancelAnimationFrame', { configurable: true, value: caf });
  win.matchMedia = win.matchMedia || (() => ({ matches: false, addListener: () => {}, removeListener: () => {}, addEventListener: () => {}, removeEventListener: () => {}, dispatchEvent: () => false }));
  globalThis.matchMedia = win.matchMedia;
  (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = class { observe() {} unobserve() {} disconnect() {} };
  (globalThis as unknown as { IntersectionObserver: unknown }).IntersectionObserver = class { observe() {} unobserve() {} disconnect() {} };
  // Realtime transport and GPU rendering are external boundaries, not the tested call gate.
  class LocalSocket extends win.EventTarget {
    static OPEN = 1;
    static CONNECTING = 0;
    readyState = 1;
    constructor() {
      super();
      queueMicrotask(() => { if (this.readyState === 1) this.dispatchEvent(new win.Event('open')); });
    }
    send() {}
    close() { this.readyState = 3; }
  }
  Object.defineProperty(globalThis, 'WebSocket', { configurable: true, value: LocalSocket });
  Object.defineProperty(win.HTMLCanvasElement.prototype, 'getContext', { configurable: true, value: () => null });
  container = win.document.getElementById('root') as HTMLElement;
}

afterAll(async () => {
  if (root) {
    await act(async () => {
      root!.unmount();
    });
    root = null;
  }
  testDom?.window.close();
  for (const [name, descriptor] of originalGlobals.entries()) {
    if (descriptor) {
      Object.defineProperty(globalThis, name, descriptor);
    } else {
      delete (globalThis as Record<string, unknown>)[name];
    }
  }
});

for (const [locale,voice] of [['zh-CN',zh],['en-US',en]] as const) {
  test(`${locale}: voice primary action is exclusive across task, draft and call states`,async()=>{
    const i18n=createInstance();await i18n.init({lng:locale,resources:{[locale]:{translation:{voice}}},initImmediate:false});
    for(const hasDraft of [false,true])for(const active of [false,true])for(const running of [false,true]){
      const html=renderToStaticMarkup(<I18nextProvider i18n={i18n}><CloudAuthContext.Provider value={{ready:false,status:'unauthenticated'} as CloudAuthContextValue}>
        <ComposerSubmitCluster isVoiceConversation hasDraft={hasDraft} isVoiceActive={active} showStop={running} onStop={()=>{}} onSend={()=>{}} onSpeechTranscript={()=>{}} onStartVoiceCall={()=>{}} onStopVoiceCall={()=>{}}/>
      </CloudAuthContext.Provider></I18nextProvider>);
      expect((html.match(/data-testid="composer-(send|stop|phone)-btn"/g)??[]).length).toBe(1);
      expect(html).toContain(`data-testid="composer-${running?'stop':hasDraft?'send':'phone'}-btn"`);
      if(!running&&!hasDraft)expect(html).toContain(active?voice.session.hangup:voice.session.startCall);
      if(!running&&!hasDraft&&active)expect(html).toContain('arco-btn-status-danger');
      expect(html).not.toContain('voice.session.');
    }
  });
}

for(const [locale,voice] of [['zh-CN',zh],['en-US',en]] as const){
 test(`${locale}: uncertain startup remains recoverable and cannot send or hang up an unbound call`,async()=>{
  const i18n=createInstance();await i18n.init({lng:locale,resources:{[locale]:{translation:{voice}}},initImmediate:false});
  const html=renderToStaticMarkup(<I18nextProvider i18n={i18n}><CloudAuthContext.Provider value={{ready:false,status:'unauthenticated'} as CloudAuthContextValue}>
   <ComposerSubmitCluster isVoiceConversation hasDraft isVoiceStartUncertain onSend={()=>{}} onSpeechTranscript={()=>{}} onStartVoiceCall={()=>{}}/>
  </CloudAuthContext.Provider></I18nextProvider>);
  expect(html).toContain(voice.composer.retryStart);expect(html).not.toContain('composer-send-btn');expect(html).not.toContain('sendbox-phone-hangup-button');
 });
 test(`${locale}: active call keeps a small hangup beside the single task/send primary`,async()=>{
  const i18n=createInstance();await i18n.init({lng:locale,resources:{[locale]:{translation:{voice}}},initImmediate:false});
  for(const stop of [false,true]){
   const html=renderToStaticMarkup(<I18nextProvider i18n={i18n}><CloudAuthContext.Provider value={{ready:false,status:'unauthenticated'} as CloudAuthContextValue}>
    <ComposerSubmitCluster isVoiceConversation isVoiceActive hasDraft showStop={stop} onStop={()=>{}} onSend={()=>{}} onSpeechTranscript={()=>{}} onStopVoiceCall={()=>{}}/>
   </CloudAuthContext.Provider></I18nextProvider>);
   expect(html).toContain('composer-secondary-hangup');expect(html).toContain('arco-btn-status-danger');
   expect((html.match(/data-testid="composer-(send|stop|phone)-btn"/g)??[]).length).toBe(1);
  }
 });

  test(`${locale}: respects voiceCallLabel for accessible name across setup, checking, ready, and active states`, async () => {
    const i18n = createInstance();
    await i18n.init({ lng: locale, resources: { [locale]: { translation: { voice } } }, initImmediate: false });

    // 7 explicit Guid entry states: 未安装, 未校验, 停用, 过期, 准备失败, 就绪, 活动通话
    const prerequisiteStateMatrix = [
      { state: 'uninstalled', label: voice.entry.setup, expected: voice.entry.setup },
      { state: 'unverified', label: voice.entry.setup, expected: voice.entry.setup },
      { state: 'disabled', label: voice.entry.setup, expected: voice.entry.setup },
      { state: 'expired', label: voice.entry.setup, expected: voice.entry.setup },
      { state: 'prepare_failed', label: voice.entry.setup, expected: voice.entry.setup },
      { state: 'ready', label: voice.entry.start, expected: voice.entry.start },
      { state: 'active_call', label: voice.entry.openActive, expected: voice.entry.openActive },
    ];

    for (const { state, label, expected } of prerequisiteStateMatrix) {
      const html = renderToStaticMarkup(
        <I18nextProvider i18n={i18n}>
          <CloudAuthContext.Provider value={{ ready: false, status: 'unauthenticated' } as CloudAuthContextValue}>
            <ComposerSubmitCluster
              isVoiceConversation
              hasDraft={false}
              voiceCallLabel={label}
              onStartVoiceCall={() => {}}
              onSend={() => {}}
              onSpeechTranscript={() => {}}
            />
          </CloudAuthContext.Provider>
        </I18nextProvider>
      );
      expect(html).toContain(`aria-label="${expected}"`);
      expect(html).toContain(`title="${expected}"`);
      if (state !== 'ready') {
        expect(html).not.toContain(`aria-label="${voice.entry.start}"`);
      }
      expect(html).not.toContain(`aria-label="${voice.session.startCall}"`);
    }
  });

  test(`${locale}: prevents duplicate start/prepare during in-flight voice connection`, async () => {
    const i18n = createInstance();
    await i18n.init({ lng: locale, resources: { [locale]: { translation: { voice } } }, initImmediate: false });

    const html = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <CloudAuthContext.Provider value={{ ready: false, status: 'unauthenticated' } as CloudAuthContextValue}>
          <ComposerSubmitCluster
            isVoiceConversation
            hasDraft={false}
            isVoiceConnecting={true}
            voiceCallLabel={voice.entry.checking}
            onStartVoiceCall={() => {}}
            onSend={() => {}}
            onSpeechTranscript={() => {}}
          />
        </CloudAuthContext.Provider>
      </I18nextProvider>
    );

    // Button must be disabled and show loading to prevent repeat clicks
    expect(html).toContain('arco-btn-loading');
    expect(html).toContain('disabled');
    expect(html).toContain(`aria-label="${voice.entry.checking}"`);
  });

  (test as (name: string, run: () => Promise<void>, timeout: number) => void)(
    `${locale}: mounted GuidPage and router: 7 prerequisite states, accessible names, navigation, and single-flight debounce`,
    async () => {
    ensureDom();
    const i18n = createInstance();
    await i18n.init({
      lng: locale,
      resources: {
        [locale]: {
          translation: {
            voice,
            common: { add: locale === 'zh-CN' ? '添加' : 'Add' },
            guid: { entry: {} },
          },
        },
      },
      initImmediate: false,
    });

    const originalFetch = globalThis.fetch;
    let mockStatus = { autostart: true, running: true, models_ready: true };
    let mockCapabilities: any = { can_start_full_call: true };
    let mockPrepare = { can_start_full_call: true, reason_code: null };
    let mockStartError: any = null;
    let mockStartStatus = 400;
    let prepareGate: Promise<void> | null = null;
    let startGate: Promise<void> | null = null;
    const requests: Array<{ request_id: string; locale: string }> = [];
    let stopCalls = 0;
    let navigateRoute: ReturnType<typeof useNavigate>;
    const defer = () => {
      let release!: () => void;
      const promise = new Promise<void>((resolve) => { release = resolve; });
      return { promise, release };
    };
    async function waitFor(predicate: () => boolean) {
      const deadline = Date.now() + 2500;
      while (!predicate() && Date.now() < deadline) {
        await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
      }
      expect(predicate()).toBe(true);
    }
    function makeIdleSnapshot() {
      return {
        instance_id: `inst-idle-${++globalInstanceSeq}`,
        seq: 1,
        session_id: null,
        phase: 'idle',
        binding: null,
      };
    }
    let mockActiveSnapshot: any = makeIdleSnapshot();

    let prepareCalls = 0;
    let startCalls = 0;
    let currentNavPath = '/';

    function RouteWatcher() {
      const loc = useLocation();
      navigateRoute = useNavigate();
      currentNavPath = loc.pathname;
      return null;
    }

    globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      if (url.endsWith('/api/voice/companion/status')) {
        return Response.json(mockStatus);
      }
      if (url.endsWith('/api/voice/companion/capabilities')) {
        return Response.json(mockCapabilities);
      }
      if (url.endsWith('/api/voice/companion/prepare')) {
        prepareCalls += 1;
        if (prepareGate) await prepareGate;
        return Response.json(mockPrepare);
      }
      if (url.endsWith('/api/voice/companion/session/start')) {
        startCalls += 1;
        const reqBody = JSON.parse(String(init?.body));
        requests.push(reqBody);
        if (startGate) await startGate;
        if (mockStartError) {
          return new Response(JSON.stringify(mockStartError), {
            status: mockStartStatus,
            headers: { 'Content-Type': 'application/json' },
          });
        }
        return Response.json({
          conversation_id: 'conv-test-intent',
          request_id: reqBody?.request_id,
        });
      }
      if (url.endsWith('/api/voice/companion/session/stop')) {
        stopCalls += 1;
        expect(JSON.parse(String(init?.body)).session_id).toBe(mockActiveSnapshot.session_id);
        return Response.json({ success: true });
      }
      if (url.endsWith('/api/voice/companion/session')) {
        return Response.json(mockActiveSnapshot ?? makeIdleSnapshot());
      }
      if (url.endsWith('/api/voice/companion/events')) {
        const encoder = new TextEncoder();
        const stream = new ReadableStream({
          start(ctrl) {
            if (mockActiveSnapshot) {
              ctrl.enqueue(encoder.encode(`data: ${JSON.stringify(mockActiveSnapshot)}\n\n`));
            }
          },
        });
        return new Response(stream, { headers: { 'Content-Type': 'text/event-stream' } });
      }
      if (url.includes('/api/preset-tags') || url.includes('/api/providers') || url.includes('/api/knowledge') || url.includes('/api/mcp')) {
        return Response.json([]);
      }
      if (url.includes('/api/skills')) {
        return Response.json({ skills: [] });
      }
      if (url.includes('/api/conversations')) {
        return Response.json({ items: [], total: 0, has_more: false });
      }
      return Response.json({});
    }) as typeof fetch;

    const mockThemeContext: ThemeContextValue = {
      theme: 'light',
      themePreference: 'light',
      setThemePreference: async () => {},
      colorScheme: 'default' as any,
      setColorScheme: async () => {},
      fontScale: 1,
      setFontScale: async () => {},
    };

    async function mountGuid() {
      if (root) {
        await act(async () => {
          root!.unmount();
        });
        root = null;
      }
      container.innerHTML = '<div id="mount"></div>';
      currentNavPath = '/';
      prepareCalls = 0;
      startCalls = 0;
      requests.length = 0;

      root = createRoot(container);
      await act(async () => {
        root!.render(
          <MemoryRouter initialEntries={['/']}>
            <I18nextProvider i18n={i18n}>
              <ThemeContext.Provider value={mockThemeContext}>
                <ConversationHistoryProvider>
                  <CloudAuthContext.Provider value={{ ready: true, status: 'unauthenticated' } as CloudAuthContextValue}>
                    <RouteWatcher />
                    <Routes>
                      <Route path="/" element={<GuidPage />} />
                      <Route path="/settings/voice" element={<div data-testid="route-settings-voice" />} />
                      <Route path="/conversation/:id" element={<div data-testid="route-conversation" />} />
                    </Routes>
                  </CloudAuthContext.Provider>
                </ConversationHistoryProvider>
              </ThemeContext.Provider>
            </I18nextProvider>
          </MemoryRouter>
        );
      });
      await act(async () => {
        await new Promise((r) => setTimeout(r, 60));
      });
    }

    try {
      // 1. uninstalled (!status.models_ready) -> setup label, click navigates to /settings/voice
      mockStatus = { autostart: true, running: true, models_ready: false };
      mockCapabilities = { can_start_full_call: false, reason_code: 'models_not_installed' };
      mockActiveSnapshot = makeIdleSnapshot();
      await mountGuid();
      let btn = container.querySelector('[data-testid="composer-phone-btn"]') as HTMLButtonElement;
      expect(btn).toBeTruthy();
      expect(btn.getAttribute('aria-label')).toBe(voice.entry.setup);
      expect(btn.getAttribute('title')).toBe(voice.entry.setup);
      await act(async () => {
        btn.click();
        await new Promise((r) => setTimeout(r, 60));
      });
      expect(currentNavPath).toBe('/settings/voice');
      expect(prepareCalls).toBe(0);
      expect(startCalls).toBe(0);

      // 2. unverified (can_start_full_call: false, reason_code: 'unverified') -> setup label, navigates to /settings/voice
      mockStatus = { autostart: true, running: true, models_ready: true };
      mockCapabilities = { can_start_full_call: false, reason_code: 'unverified' };
      await mountGuid();
      btn = container.querySelector('[data-testid="composer-phone-btn"]') as HTMLButtonElement;
      expect(btn).toBeTruthy();
      expect(btn.getAttribute('aria-label')).toBe(voice.entry.setup);
      expect(btn.getAttribute('title')).toBe(voice.entry.setup);
      await act(async () => {
        btn.click();
        await new Promise((r) => setTimeout(r, 60));
      });
      expect(currentNavPath).toBe('/settings/voice');
      expect(prepareCalls).toBe(0);
      expect(startCalls).toBe(0);

      // 3. disabled (!status.autostart) -> setup label, navigates to /settings/voice
      mockStatus = { autostart: false, running: false, models_ready: true };
      mockCapabilities = { can_start_full_call: true };
      await mountGuid();
      btn = container.querySelector('[data-testid="composer-phone-btn"]') as HTMLButtonElement;
      expect(btn).toBeTruthy();
      expect(btn.getAttribute('aria-label')).toBe(voice.entry.setup);
      expect(btn.getAttribute('title')).toBe(voice.entry.setup);
      await act(async () => {
        btn.click();
        await new Promise((r) => setTimeout(r, 60));
      });
      expect(currentNavPath).toBe('/settings/voice');
      expect(prepareCalls).toBe(0);
      expect(startCalls).toBe(0);

      // 4. expired (reason_code: 'self_check_required') -> start label, click prepares then starts call
      mockStatus = { autostart: true, running: true, models_ready: true };
      mockCapabilities = {
        can_start_full_call: false,
        reason_code: 'self_check_required',
        llm: { status: 'unknown', reason_code: 'llm_check_not_run' },
      };
      mockPrepare = { can_start_full_call: true, reason_code: null };
      mockStartError = null;
      await mountGuid();
      btn = container.querySelector('[data-testid="composer-phone-btn"]') as HTMLButtonElement;
      expect(btn).toBeTruthy();
      expect(btn.getAttribute('aria-label')).toBe(voice.entry.start);
      expect(btn.getAttribute('title')).toBe(voice.entry.start);
      await act(async () => {
        btn.click();
        await new Promise((r) => setTimeout(r, 60));
      });
      expect(prepareCalls).toBe(1);
      expect(startCalls).toBe(1);
      expect(currentNavPath).toBe('/conversation/conv-test-intent');

      // 5. prepare_failed -> start label, prepare fails, navigates to /settings/voice
      mockStatus = { autostart: true, running: true, models_ready: true };
      mockCapabilities = {
        can_start_full_call: false,
        reason_code: 'self_check_required',
        llm: { status: 'unknown', reason_code: 'llm_check_not_run' },
      };
      mockPrepare = { can_start_full_call: false, reason_code: 'model_load_failed' as any };
      await mountGuid();
      btn = container.querySelector('[data-testid="composer-phone-btn"]') as HTMLButtonElement;
      expect(btn).toBeTruthy();
      expect(btn.getAttribute('aria-label')).toBe(voice.entry.start);
      expect(btn.getAttribute('title')).toBe(voice.entry.start);
      await act(async () => {
        btn.click();
        await new Promise((r) => setTimeout(r, 60));
      });
      expect(prepareCalls).toBe(1);
      expect(startCalls).toBe(0);
      expect(currentNavPath).toBe('/settings/voice');

      // 6. ready -> start label, click directly starts call without prepare
      mockStatus = { autostart: true, running: true, models_ready: true };
      mockCapabilities = { can_start_full_call: true };
      await mountGuid();
      btn = container.querySelector('[data-testid="composer-phone-btn"]') as HTMLButtonElement;
      expect(btn).toBeTruthy();
      expect(btn.getAttribute('aria-label')).toBe(voice.entry.start);
      expect(btn.getAttribute('title')).toBe(voice.entry.start);
      await act(async () => {
        btn.click();
        await new Promise((r) => setTimeout(r, 60));
      });
      expect(prepareCalls).toBe(0);
      expect(startCalls).toBe(1);
      expect(currentNavPath).toBe('/conversation/conv-test-intent');

      // 7. active_call -> openActive label, navigates to active conversation
      mockStatus = { autostart: false, running: true, models_ready: false };
      mockCapabilities = null;
      mockActiveSnapshot = {
        instance_id: `inst-active-${++globalInstanceSeq}`,
        seq: 1,
        session_id: 'active-session-id',
        phase: 'speaking',
        binding: { conversation_id: 'conv-active-current' },
      };
      await mountGuid();
      btn = container.querySelector('[data-testid="composer-phone-btn"]') as HTMLButtonElement;
      expect(btn).toBeTruthy();
      expect(btn.getAttribute('aria-label')).toBe(voice.entry.openActive);
      expect(btn.getAttribute('title')).toBe(voice.entry.openActive);
      await act(async () => {
        btn.click();
        await new Promise((r) => setTimeout(r, 60));
      });
      expect(prepareCalls).toBe(0);
      expect(startCalls).toBe(0);
      expect(currentNavPath).toBe('/conversation/conv-active-current');

      // 8. Single-flight debounce on rapid double-click in ready state
      mockActiveSnapshot = makeIdleSnapshot();
      mockStatus = { autostart: true, running: true, models_ready: true };
      mockCapabilities = { can_start_full_call: true };
      await mountGuid();
      btn = container.querySelector('[data-testid="composer-phone-btn"]') as HTMLButtonElement;
      expect(btn).toBeTruthy();
      expect(btn.getAttribute('aria-label')).toBe(voice.entry.start);
      startCalls = 0;
      await act(async () => {
        btn.click();
        btn.click(); // Immediate consecutive click while inFlightVoiceCallRef is true
        await new Promise((r) => setTimeout(r, 60));
      });
      expect(startCalls).toBe(1); // Exactly one call dispatched

      // The same gate owns preparation, not just the final start request.
      mockCapabilities = null;
      mockPrepare = { can_start_full_call: true, reason_code: null };
      const preparation = defer();
      prepareGate = preparation.promise;
      await mountGuid();
      btn = container.querySelector('[data-testid="composer-phone-btn"]') as HTMLButtonElement;
      await act(async () => { btn.click(); btn.click(); });
      await waitFor(() => prepareCalls === 1);
      expect(startCalls).toBe(0);
      await act(async () => { preparation.release(); });
      prepareGate = null;
      await waitFor(() => currentNavPath === '/conversation/conv-test-intent');
      expect(prepareCalls).toBe(1);
      expect(startCalls).toBe(1);
      expect(requests[0].locale).toBe(locale);

      // A 503 is unknown, so retry must reuse the original request identity.
      mockCapabilities = { can_start_full_call: true };
      mockStartError = { code: 'daemon_unavailable' };
      mockStartStatus = 503;
      await mountGuid();
      btn = container.querySelector('[data-testid="composer-phone-btn"]') as HTMLButtonElement;
      await act(async () => { btn.click(); });
      await waitFor(() => startCalls === 1 && !btn.disabled);
      expect(currentNavPath).toBe('/');
      const unknownId = requests[0].request_id;
      expect(getPendingVoiceCallIntent()?.request_id).toBe(unknownId);
      mockStartError = null;
      await act(async () => { btn.click(); });
      await waitFor(() => currentNavPath === '/conversation/conv-test-intent');
      expect(requests.map((request) => request.request_id)).toEqual([unknownId, unknownId]);

      // Leaving during preparation must not create a call after it resolves.
      mockCapabilities = null;
      const leavingPrepare = defer();
      prepareGate = leavingPrepare.promise;
      await mountGuid();
      btn = container.querySelector('[data-testid="composer-phone-btn"]') as HTMLButtonElement;
      await act(async () => { btn.click(); });
      await waitFor(() => prepareCalls === 1);
      await act(async () => { navigateRoute('/settings/voice'); });
      await act(async () => { leavingPrepare.release(); });
      prepareGate = null;
      expect(startCalls).toBe(0);
      expect(currentNavPath).toBe('/settings/voice');

      // Leaving after start was sent must cancel exactly the owned session.
      mockCapabilities = { can_start_full_call: true };
      const leavingStart = defer();
      startGate = leavingStart.promise;
      await mountGuid();
      btn = container.querySelector('[data-testid="composer-phone-btn"]') as HTMLButtonElement;
      await act(async () => { btn.click(); });
      await waitFor(() => startCalls === 1);
      await act(async () => { navigateRoute('/settings/voice'); });
      mockActiveSnapshot = {
        ...makeIdleSnapshot(), call_request_id: requests[0].request_id,
        session_id: 'owned-left-session', phase: 'listening', turn_id: 'owned-turn',
      };
      await act(async () => { leavingStart.release(); });
      startGate = null;
      await waitFor(() => getPendingVoiceCallIntent() === null);
      expect(stopCalls).toBe(1);
      expect(startCalls).toBe(1);
      expect(currentNavPath).toBe('/settings/voice');
    } finally {
      if (root) {
        await act(async () => {
          root!.unmount();
        });
        root = null;
      }
      globalThis.fetch = originalFetch;
    }
  }, 30_000);
}
