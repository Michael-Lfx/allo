import * as bunTest from 'bun:test';
const { describe, expect, test } = bunTest;
const afterAll = (bunTest as unknown as { afterAll: (callback: () => void) => void }).afterAll;
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
// @ts-expect-error jsdom has no local declaration in this workspace
import { JSDOM } from 'jsdom';
import SpeechInputButton, { type DictationActivationResult } from './SpeechInputButton';
import { MemoryRouter, Route, Routes, useLocation, useNavigate } from 'react-router-dom';
import { DictationIntentRouteGuard } from '@/renderer/components/layout/DictationIntentRouteGuard';
import { consumeDictationIntent, pendingDictationIntent, requestDictationIntent, revokeDictationIntent } from '@/renderer/pages/guid/dictationIntent';

let root: Root | null = null;
let container: HTMLElement;
let getUserMediaCalls = 0;
let stopCalls = 0;

class FakeMediaRecorder {
  static isTypeSupported() { return true; }
  state = 'inactive';
  mimeType = 'audio/webm';
  ondataavailable: ((event: { data: Blob }) => void) | null = null;
  onerror: (() => void) | null = null;
  onstop: (() => void) | null = null;
  constructor(public stream: unknown, public options?: unknown) {}
  start() { this.state = 'recording'; }
  stop() { stopCalls += 1; this.state = 'inactive'; }
}

class FakeAudioContext {
  createAnalyser() {
    return {
      fftSize: 128,
      smoothingTimeConstant: 0.82,
      getByteTimeDomainData() {},
      disconnect() {},
    };
  }
  createMediaStreamSource() { return { connect() {}, disconnect() {} }; }
  async close() {}
}

const globalsToRestore = ["Element","HTMLElement","Node","Event","MouseEvent","KeyboardEvent","DocumentFragment","Blob","File","window","document","navigator","MediaRecorder","AudioContext","IS_REACT_ACT_ENVIRONMENT"] as const;
const originalGlobals = new Map<string, PropertyDescriptor | undefined>();
for (const name of globalsToRestore) originalGlobals.set(name, Object.getOwnPropertyDescriptor(globalThis, name));
const dom = (() => {
  const domInstance = new JSDOM('<!doctype html><html><body><div id="root"></div></body></html>', { url: 'http://localhost/' });
  const win = domInstance.window as unknown as Window & typeof globalThis;
  (win as unknown as { isSecureContext: boolean }).isSecureContext = true;
  Object.defineProperty(win.navigator, 'mediaDevices', {
    configurable: true,
    value: {
      getUserMedia: async () => {
        getUserMediaCalls += 1;
        return { getTracks: () => [{ stop() {} }] };
      },
    },
  });
  for (const name of ['Element','HTMLElement','Node','Event','MouseEvent','KeyboardEvent','DocumentFragment','Blob','File'] as const) {
    Object.defineProperty(globalThis, name, { configurable: true, value: (win as unknown as Record<string, unknown>)[name] });
  }
  Object.defineProperty(globalThis, 'window', { configurable: true, value: win });
  Object.defineProperty(globalThis, 'document', { configurable: true, value: win.document });
  Object.defineProperty(globalThis, 'navigator', { configurable: true, value: win.navigator });
  Object.defineProperty(globalThis, 'MediaRecorder', { configurable: true, value: FakeMediaRecorder });
  Object.defineProperty(globalThis, 'AudioContext', { configurable: true, value: FakeAudioContext });
  Object.defineProperty(globalThis, 'IS_REACT_ACT_ENVIRONMENT', { configurable: true, value: true });
  container = win.document.getElementById('root') as HTMLElement;
  return domInstance;
})();



function reset() {
  root?.unmount();
  root = null;
  container.innerHTML = '<div id="mount"></div>';
  getUserMediaCalls = 0;
  stopCalls = 0;
}

async function render(element: React.ReactElement) {
  root = createRoot(container);
  await act(async () => { root!.render(element); });
  await act(async () => { await Promise.resolve(); });
}

describe('SpeechInputButton owned dictation activation', () => {
  test('a delayed mounted control consumes the pending activation once', async () => {
    reset();
    const results: DictationActivationResult[] = [];
    function Delayed() {
      const [shown, setShown] = React.useState(false);
      React.useEffect(() => {
        const timer = setTimeout(() => setShown(true), 60);
        return () => clearTimeout(timer);
      }, []);
      return shown
        ? <SpeechInputButton activationRequest={{ id: 'late', claim: () => true, onResult: (result) => results.push(result) }} onTranscript={() => {}} />
        : null;
    }
    await render(<Delayed />);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 100)); });
    expect(results).toEqual(['started']);
    expect(getUserMediaCalls).toBe(1);
  });

  function HomeConsumer() {
    const location = useLocation();
    const allowedId = (location.state as { dictationIntentId?: string } | null)?.dictationIntentId;
    const [intent, setIntent] = React.useState<{ id: string } | null>(null);
    React.useEffect(() => {
      const pending = pendingDictationIntent();
      if (pending?.route === '/' && pending.id === allowedId) setIntent({ id: pending.id });
    }, [allowedId]);
    if (!intent) return <div>no-intent</div>;
    return (
      <SpeechInputButton
        activationRequest={{
          id: intent.id,
          claim: () => consumeDictationIntent('/', intent.id),
          onResult: () => {},
        }}
        onTranscript={() => {}}
      />
    );
  }

  function RouterDriver({ withIntent, returnWithState }: { withIntent: boolean; returnWithState: boolean }) {
    const navigate = useNavigate();
    React.useEffect(() => {
      const intent = withIntent ? requestDictationIntent('/') : null;
      if (!returnWithState) navigate('/other');
      const timer = setTimeout(() => {
        navigate('/', returnWithState && intent ? { state: { dictationIntentId: intent.id } } : undefined);
      }, 30);
      return () => clearTimeout(timer);
    }, [returnWithState, withIntent]);
    return (
      <>
        <DictationIntentRouteGuard />
        <Routes>
          <Route path='/settings' element={<div>settings</div>} />
          <Route path='/other' element={<div>other</div>} />
          <Route path='/' element={<HomeConsumer />} />
        </Routes>
      </>
    );
  }

  test('leaving before home mounts revokes the intent across the real router', async () => {
    reset();
    await render(
      <MemoryRouter initialEntries={['/settings']}>
        <RouterDriver withIntent returnWithState={false} />
      </MemoryRouter>,
    );
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 120)); });
    expect(container.textContent).toContain('no-intent');
    expect(getUserMediaCalls).toBe(0);
  });

  test('a normal return without a matching navigation state does not consume a stale intent', async () => {
    reset();
    const intent = requestDictationIntent('/');
    await render(
      <MemoryRouter initialEntries={['/']}>
        <DictationIntentRouteGuard />
        <HomeConsumer />
      </MemoryRouter>,
    );
    await act(async () => { await Promise.resolve(); });
    expect(pendingDictationIntent()).toBeNull();
    expect(getUserMediaCalls).toBe(0);
    revokeDictationIntent(intent.id);
  });

  test('a matching navigation state still starts dictation after a delayed mount', async () => {
    reset();
    await render(
      <MemoryRouter initialEntries={['/settings']}>
        <RouterDriver withIntent returnWithState />
      </MemoryRouter>,
    );
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 120)); });
    expect(getUserMediaCalls).toBe(1);
  });

  test('normal manual microphone clicks still start recording without any intent', async () => {
    reset();
    await render(<SpeechInputButton onTranscript={() => {}} />);
    const microphone = container.querySelector('[data-layout-part="microphone"]') as HTMLElement | null;
    expect(microphone).toBeTruthy();
    await act(async () => { microphone!.click(); await Promise.resolve(); });
    expect(getUserMediaCalls).toBe(1);
  });

  test('a failed atomic claim prevents recording even when the control mounted after TTL', async () => {
    reset();
    const results: DictationActivationResult[] = [];
    await render(
      <SpeechInputButton activationRequest={{ id: 'expired', claim: () => false, onResult: (result) => results.push(result) }} onTranscript={() => {}} />,
    );
    expect(results).toEqual(['stale']);
    expect(getUserMediaCalls).toBe(0);
  });

  test('an unavailable control reports failure without opening the microphone', async () => {
    reset();
    const results: DictationActivationResult[] = [];
    await render(
      <SpeechInputButton disabled activationRequest={{ id: 'disabled', claim: () => true, onResult: (result) => results.push(result) }} onTranscript={() => {}} />,
    );
    expect(results).toEqual(['unavailable']);
    expect(getUserMediaCalls).toBe(0);
  });

  test('a duplicate activation never stops an existing recording', async () => {
    reset();
    const first: DictationActivationResult[] = [];
    const second: DictationActivationResult[] = [];
    root = createRoot(container);
    await act(async () => {
      root!.render(<SpeechInputButton activationRequest={{ id: 'first', claim: () => true, onResult: (result) => first.push(result) }} onTranscript={() => {}} />);
    });
    await act(async () => { await Promise.resolve(); });
    expect(first).toEqual(['started']);
    expect(getUserMediaCalls).toBe(1);
    await act(async () => {
      root!.render(<SpeechInputButton activationRequest={{ id: 'second', claim: () => true, onResult: (result) => second.push(result) }} onTranscript={() => {}} />);
    });
    await act(async () => { await Promise.resolve(); });
    expect(second).toEqual(['already-recording']);
    expect(stopCalls).toBe(0);
  });
});
