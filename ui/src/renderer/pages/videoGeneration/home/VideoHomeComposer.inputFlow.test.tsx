import * as bunTest from 'bun:test';
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
// @ts-expect-error jsdom has no local declaration in this workspace
import { JSDOM } from 'jsdom';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import { MemoryRouter } from 'react-router-dom';
import { buildHomeVideoGenerateRequest } from '@renderer/pages/guid/homeVideoSubmit';
import type { VideoCreateDraft } from './types';
import VideoHomeComposer from './VideoHomeComposer';
import { DRAFT_KEY, defaultDraft, serializeDraft } from './homeDraft';
import videoZh from '@renderer/services/i18n/locales/zh-CN/videoGeneration.json';
import { ThemeContext, type ThemeContextValue } from '@renderer/hooks/context/ThemeContext';
import { CloudAuthContext } from '@renderer/hooks/context/CloudAuthContext';
import { CreditsContext } from '@renderer/hooks/context/CreditsContext';

const { afterAll, beforeAll, describe, expect, test } = bunTest as unknown as {
  afterAll: (callback: () => void | Promise<void>) => void;
  beforeAll: (callback: () => void | Promise<void>) => void;
  describe: (name: string, callback: () => void) => void;
  expect: typeof bunTest.expect;
  test: (name: string, callback: () => void | Promise<void>) => void;
};

const INPUT_TEXT = '一只猫在窗边伸懒腰，镜头缓慢推近';

describe('home video composer input flow', () => {
  let dom: JSDOM;
  let container: HTMLDivElement;
  let root: Root;

  beforeAll(() => {
    dom = new JSDOM('<!doctype html><html><body><div id="root"></div></body></html>', {
      url: 'http://localhost/',
    });
    Object.defineProperty(globalThis, 'window', { configurable: true, value: dom.window });
    Object.defineProperty(globalThis, 'document', { configurable: true, value: dom.window.document });
    Object.defineProperty(globalThis, 'navigator', { configurable: true, value: dom.window.navigator });
    for (const name of [
      'Element',
      'HTMLElement',
      'HTMLTextAreaElement',
      'HTMLInputElement',
      'HTMLButtonElement',
      'SVGElement',
      'Node',
      'Event',
      'CustomEvent',
      'InputEvent',
      'MouseEvent',
      'KeyboardEvent',
      'DocumentFragment',
    ] as const) {
      Object.defineProperty(globalThis, name, {
        configurable: true,
        value: (dom.window as unknown as Record<string, unknown>)[name],
      });
    }
    Object.defineProperty(globalThis, 'IS_REACT_ACT_ENVIRONMENT', { configurable: true, value: true });
    // React 19 probes the legacy attachEvent path in jsdom while dispatching
    // input events; provide inert shims so the synthetic handler can run.
    Object.defineProperty(dom.window.HTMLElement.prototype, 'attachEvent', {
      configurable: true,
      value: () => undefined,
    });
    Object.defineProperty(dom.window.HTMLElement.prototype, 'detachEvent', {
      configurable: true,
      value: () => undefined,
    });
    const raf = (cb: FrameRequestCallback) => setTimeout(() => cb(Date.now()), 16) as unknown as number;
    dom.window.requestAnimationFrame = raf;
    dom.window.cancelAnimationFrame = (id: number) => clearTimeout(id);
    dom.window.matchMedia = () => ({
      matches: false,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
      addListener: () => undefined,
      removeListener: () => undefined,
      onchange: null,
      dispatchEvent: () => false,
      media: '',
    });
    class ResizeObserverStub {
      observe() {}
      unobserve() {}
      disconnect() {}
    }
    Object.defineProperty(globalThis, 'ResizeObserver', { configurable: true, value: ResizeObserverStub });
    Object.defineProperty(globalThis, 'IntersectionObserver', {
      configurable: true,
      value: class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    });

    const seeded: VideoCreateDraft = {
      ...defaultDraft(),
      preferences: {
        ...defaultDraft().preferences,
        models: { ...defaultDraft().preferences.models, video_model: 'minimax-h3' },
      },
    };
    dom.window.sessionStorage.setItem(DRAFT_KEY, JSON.stringify(serializeDraft(seeded)));
    container = dom.window.document.getElementById('root') as HTMLDivElement;
    root = createRoot(container);
  });

  afterAll(async () => {
    await act(async () => root.unmount());
    dom.window.close();
  });

  test('typing in the composer reaches the generate submit draft', async () => {
    const i18n = createInstance();
    await i18n.init({
      lng: 'zh-CN',
      resources: { 'zh-CN': { translation: { videoGeneration: videoZh } } },
      initImmediate: false,
    });
    const theme: ThemeContextValue = {
      theme: 'dark',
      themePreference: 'dark',
      setThemePreference: async () => undefined,
      colorScheme: 'codex-neutral' as ThemeContextValue['colorScheme'],
      setColorScheme: async () => undefined,
      fontScale: 1,
      setFontScale: async () => undefined,
    };
    const submissions: VideoCreateDraft[] = [];

    await act(async () => {
      root.render(
        <I18nextProvider i18n={i18n}>
          <ThemeContext.Provider value={theme}>
            <CloudAuthContext.Provider value={{ ready: false, status: 'unauthenticated' } as never}>
              <CreditsContext.Provider value={{ balance: 100 } as never}>
                <MemoryRouter>
                  <VideoHomeComposer
                    mode='generate'
                    onModeChange={() => undefined}
                    onSubmitAgent={() => undefined}
                    onSubmitCreation={() => undefined}
                    onSubmitGenerate={(draft) => submissions.push(draft)}
                    onSubmitBriefing={() => undefined}
                    onCreateBlankCanvas={() => undefined}
                    showHero={false}
                    showCampaign={false}
                  />
                </MemoryRouter>
              </CreditsContext.Provider>
            </CloudAuthContext.Provider>
          </ThemeContext.Provider>
        </I18nextProvider>,
      );
    });

    const textarea = container.querySelector('textarea');
    expect(textarea).not.toBeNull();

    // The composer surface must follow the app theme tokens (light/dark plus
    // CSS skins) instead of an inline canvas-editor palette. A regression here
    // means light themes can leave the input box dark.
    const composerShell = container.querySelector('.canvas-composer-shell') as HTMLElement | null;
    expect(composerShell).not.toBeNull();
    expect(composerShell?.getAttribute('style')).toBeNull();
    // jsdom 30 + React 19 does not route a native textarea input through the
    // same onChange path as WebView2. Invoke the public onChange prop directly
    // so this test still locks the real composer state -> submit boundary.
    const reactPropsKey = textarea ? Object.keys(textarea).find((key) => key.startsWith('__reactProps')) : undefined;
    expect(reactPropsKey).toBeDefined();
    const reactProps = reactPropsKey
      ? ((textarea as unknown as Record<string, { onChange?: (event: unknown) => void; onKeyDown?: (event: unknown) => void }>)[
          reactPropsKey
        ])
      : undefined;
    expect(typeof reactProps?.onChange).toBe('function');

    // Ctrl/Cmd+Enter must not bypass the empty-prompt guard.
    await act(async () => {
      reactProps?.onKeyDown?.({ key: 'Enter', ctrlKey: true, preventDefault: () => undefined, target: textarea });
    });
    expect(submissions).toHaveLength(0);

    const valueSetter = Object.getOwnPropertyDescriptor(dom.window.HTMLTextAreaElement.prototype, 'value')?.set;
    await act(async () => {
      valueSetter?.call(textarea, INPUT_TEXT);
      reactProps?.onChange?.({ target: textarea, currentTarget: textarea });
    });

    const submit = container.querySelector('[data-video-home-submit]') as HTMLButtonElement | null;
    expect(submit).not.toBeNull();
    expect(submit?.disabled).toBe(false);
    await act(async () => {
      submit?.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
    });

    expect(submissions).toHaveLength(1);
    expect(submissions[0].creationPrompt).toBe(INPUT_TEXT);
    expect(submissions[0].preferences.models.video_model).toBe('minimax-h3');

    const request = buildHomeVideoGenerateRequest(submissions[0]);
    expect(request.prompt).toContain(INPUT_TEXT);
    expect(request.title).toBe(INPUT_TEXT);
    expect(request.model).toBe('minimax-h3');
  });
});
