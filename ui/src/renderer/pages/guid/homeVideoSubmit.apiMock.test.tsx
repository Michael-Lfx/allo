import * as bunTest from 'bun:test';
// @ts-expect-error jsdom has no local declaration in this workspace
import { JSDOM } from 'jsdom';

const { afterAll, describe, expect, mock, test } = bunTest as unknown as {
  afterAll: (callback: () => void | Promise<void>) => void;
  describe: (name: string, callback: () => void) => void;
  expect: typeof bunTest.expect;
  mock: typeof bunTest.mock & { restore: () => void };
  test: (name: string, callback: () => void | Promise<void>) => void;
};



const dom = new JSDOM('<!doctype html><html><body><div id="root"></div></body></html>', {
  url: 'http://localhost/',
});
Object.defineProperty(globalThis, 'window', { configurable: true, value: dom.window });
Object.defineProperty(globalThis, 'document', { configurable: true, value: dom.window.document });
Object.defineProperty(globalThis, 'navigator', { configurable: true, value: dom.window.navigator });
Object.defineProperty(globalThis, 'location', { configurable: true, value: dom.window.location });
Object.defineProperty(globalThis, 'IS_REACT_ACT_ENVIRONMENT', { configurable: true, value: true });
for (const name of ['Element', 'HTMLElement', 'File', 'Node', 'Event', 'CustomEvent', 'MouseEvent', 'KeyboardEvent'] as const) {
  Object.defineProperty(globalThis, name, {
    configurable: true,
    value: (dom.window as unknown as Record<string, unknown>)[name],
  });
}

const realVideoCanvasApi = await import('../videoCanvas/api');
const realCloudAuth = await import('@renderer/hooks/context/CloudAuthContext');
const realCredits = await import('@renderer/hooks/context/CreditsContext');
const realArcoMessage = await import('@renderer/utils/ui/useArcoMessage');

const createCalls: unknown[] = [];
const uploadCalls: unknown[] = [];
const createTask = async (payload: unknown) => {
  createCalls.push(payload);
  return { task_id: 'task-1' } as never;
};
const uploadMedia = async (...args: unknown[]) => {
  uploadCalls.push(args);
  return { media_id: 'media-1' } as never;
};
const deleteMedia = async () => undefined;
mock.module('../videoCanvas/api', () => ({
  ...realVideoCanvasApi,
  createGenerationTask: createTask,
  uploadCanvasMedia: uploadMedia,
  deleteCanvasMedia: deleteMedia,
  listGenerationTasks: async () => ({ tasks: [], total: 0 }),
}));
mock.module('@renderer/utils/ui/useArcoMessage', () => ({
  ...realArcoMessage,
  useArcoMessage: () => [{ warning: () => undefined, error: () => undefined }, null],
}));
mock.module('@renderer/hooks/context/CloudAuthContext', () => ({
  ...realCloudAuth,
  useCloudAuth: () => ({ logout: async () => undefined }),
}));
mock.module('@renderer/hooks/context/CreditsContext', () => ({
  ...realCredits,
  useCredits: () => ({ balance: 100 }),
}));

describe('home video submit API mock', () => {
  test('generation receives the prompt and uploaded media ids through the real API boundary', async () => {
    const React = await import('react');
    const { act } = React;
    const { createRoot } = await import('react-dom/client');
    const { createInstance } = await import('i18next');
    const { I18nextProvider } = await import('react-i18next');
    const { MemoryRouter } = await import('react-router-dom');
    const { useHomeVideoSubmit } = await import('./homeVideoSubmit');
    const { defaultDraft: makeDefaultDraft } = await import('../videoGeneration/home/homeDraft');

    const i18n = createInstance();
    await i18n.init({ lng: 'zh-CN', resources: { 'zh-CN': { translation: {} } }, initImmediate: false });
    const container = dom.window.document.getElementById('root') as HTMLDivElement;
    const root = createRoot(container);
    let api: ReturnType<typeof useHomeVideoSubmit> | undefined;
    const Harness = () => {
      api = useHomeVideoSubmit();
      return null;
    };

    await act(async () => {
      root.render(
        React.createElement(
          I18nextProvider,
          { i18n },
          React.createElement(
            MemoryRouter,
            null,
            React.createElement(Harness),
          ),
        ),
      );
    });

    const draft = {
      ...makeDefaultDraft(),
      creationPrompt: '一只猫在窗边伸懒腰，镜头缓慢推近',
      preferences: {
        ...makeDefaultDraft().preferences,
        models: { ...makeDefaultDraft().preferences.models, video_model: 'minimax-h3' },
      },
      canvasReferences: [
        {
          localId: 'ref-1',
          file: new dom.window.File(['image'], 'reference.png', { type: 'image/png' }),
          previewUrl: 'blob:ref',
        },
      ],
    };

    await act(async () => {
      await api?.handleCreateGenerate(draft);
    });

    expect(createCalls).toHaveLength(1);
    const payload = createCalls[0] as Record<string, unknown>;
    expect(payload.mode).toBe('video');
    expect(payload.prompt).toContain('一只猫在窗边伸懒腰');
    expect(payload.model).toBe('minimax-h3');
    expect(payload.reference_media_ids).toEqual(['media-1']);
    expect(uploadCalls).toHaveLength(1);

    await act(async () => root.unmount());
    i18n.removeResourceBundle('zh-CN', 'translation');
    dom.window.close();
  });

  afterAll(() => {
    mock.restore();
  });
});
