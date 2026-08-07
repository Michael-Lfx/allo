import { describe, expect, test } from 'bun:test';
import { PasteService } from './PasteService';

class FakeNode {
  parentElement: FakeElement | null;

  constructor(parentElement: FakeElement | null = null) {
    this.parentElement = parentElement;
  }
}

class FakeElement extends FakeNode {
  readonly tagName: string;
  readonly isContentEditable: boolean;
  private readonly editableAncestor: FakeElement | null;
  private readonly contentEditableValue: string | null;

  constructor(
    tagName = 'DIV',
    options: {
      editableAncestor?: FakeElement | null;
      contentEditable?: string | null;
      isContentEditable?: boolean;
    } = {}
  ) {
    super();
    this.tagName = tagName;
    this.editableAncestor = options.editableAncestor ?? null;
    this.contentEditableValue = options.contentEditable ?? null;
    this.isContentEditable =
      options.isContentEditable ?? (this.contentEditableValue !== null && this.contentEditableValue !== 'false');
  }

  closest() {
    if (this.editableAncestor) {
      return this.editableAncestor;
    }
    if (this.tagName === 'INPUT' || this.tagName === 'TEXTAREA' || this.contentEditableValue !== null) {
      return this;
    }
    return null;
  }

  getAttribute(name: string) {
    return name === 'contenteditable' ? this.contentEditableValue : null;
  }
}

class FakeInputElement extends FakeElement {
  constructor() {
    super('INPUT');
  }
}

class FakeTextAreaElement extends FakeElement {
  constructor() {
    super('TEXTAREA');
  }
}

class FakeTextNode extends FakeNode {}

type PasteListener = (event: ClipboardEvent) => Promise<void> | void;

const originalGlobals = {
  document: globalThis.document,
  Element: globalThis.Element,
  Node: globalThis.Node,
  HTMLElement: globalThis.HTMLElement,
  HTMLInputElement: globalThis.HTMLInputElement,
  HTMLTextAreaElement: globalThis.HTMLTextAreaElement,
};

let pasteListener: PasteListener | null = null;

function installFakeBrowserGlobals() {
  pasteListener = null;
  Object.defineProperty(globalThis, 'Element', { configurable: true, value: FakeElement });
  Object.defineProperty(globalThis, 'Node', { configurable: true, value: FakeNode });
  Object.defineProperty(globalThis, 'HTMLElement', { configurable: true, value: FakeElement });
  Object.defineProperty(globalThis, 'HTMLInputElement', { configurable: true, value: FakeInputElement });
  Object.defineProperty(globalThis, 'HTMLTextAreaElement', { configurable: true, value: FakeTextAreaElement });
  Object.defineProperty(globalThis, 'document', {
    configurable: true,
    value: {
      addEventListener: (type: string, listener: PasteListener) => {
        if (type === 'paste') {
          pasteListener = listener;
        }
      },
      removeEventListener: (type: string, listener: PasteListener) => {
        if (type === 'paste' && pasteListener === listener) {
          pasteListener = null;
        }
      },
    },
  });
}

function restoreGlobal(name: keyof typeof originalGlobals) {
  const value = originalGlobals[name];
  if (value === undefined) {
    Reflect.deleteProperty(globalThis, name);
  } else {
    Object.defineProperty(globalThis, name, { configurable: true, value });
  }
}

function restoreFakeBrowserGlobals() {
  PasteService.destroy();
  restoreGlobal('document');
  restoreGlobal('Element');
  restoreGlobal('Node');
  restoreGlobal('HTMLElement');
  restoreGlobal('HTMLInputElement');
  restoreGlobal('HTMLTextAreaElement');
}

async function withFakeBrowserGlobals(callback: () => void | Promise<void>) {
  installFakeBrowserGlobals();
  try {
    await callback();
  } finally {
    restoreFakeBrowserGlobals();
  }
}

function createPasteEvent(
  target: unknown,
  path: unknown[] = [target],
  defaultPrevented = false
): ClipboardEvent {
  const event = {
    target,
    get defaultPrevented() {
      return defaultPrevented;
    },
    composedPath: () => path,
    preventDefault() {
      defaultPrevented = true;
    },
    stopPropagation() {},
  } as unknown as ClipboardEvent;
  return event;
}

async function dispatchGlobalPaste(event: ClipboardEvent) {
  expect(pasteListener).not.toBeNull();
  await pasteListener!(event);
}

describe('PasteService global paste routing', () => {
  test('does not route an already handled event', () =>
    withFakeBrowserGlobals(async () => {
      let handlerCalls = 0;
      PasteService.registerHandler('composer', async () => {
        handlerCalls += 1;
        return false;
      });
      PasteService.setLastFocusedComponent('composer');
      PasteService.init();

      await dispatchGlobalPaste(createPasteEvent(new FakeElement(), [new FakeElement()], true));

      expect(handlerCalls).toBe(0);
    })
  );

  test('leaves input, textarea, and contentEditable targets alone', () =>
    withFakeBrowserGlobals(async () => {
      let handlerCalls = 0;
      PasteService.registerHandler('composer', async () => {
        handlerCalls += 1;
        return false;
      });
      PasteService.setLastFocusedComponent('composer');
      PasteService.init();

      const targets = [
        new FakeInputElement(),
        new FakeTextAreaElement(),
        new FakeElement('DIV', { contentEditable: '' }),
        new FakeElement('DIV', { contentEditable: 'true' }),
        new FakeElement('DIV', { contentEditable: 'plaintext-only' }),
      ];
      for (const target of targets) {
        await dispatchGlobalPaste(createPasteEvent(target));
      }

      expect(handlerCalls).toBe(0);
    })
  );

  test('recognizes a text node inside contentEditable through the composed path', () =>
    withFakeBrowserGlobals(async () => {
      let handlerCalls = 0;
      PasteService.registerHandler('composer', async () => {
        handlerCalls += 1;
        return false;
      });
      PasteService.setLastFocusedComponent('composer');
      PasteService.init();

      const root = new FakeElement('DIV', { contentEditable: 'true' });
      const child = new FakeElement('SPAN', { editableAncestor: root });
      const textNode = new FakeTextNode(child);
      await dispatchGlobalPaste(createPasteEvent(textNode, [textNode, child, root]));

      expect(handlerCalls).toBe(0);
    })
  );

  test('routes a file paste from a non-editable target to the focused handler once', () =>
    withFakeBrowserGlobals(async () => {
      let handlerCalls = 0;
      PasteService.registerHandler('composer', async () => {
        handlerCalls += 1;
        return false;
      });
      PasteService.setLastFocusedComponent('composer');
      PasteService.init();

      await dispatchGlobalPaste(createPasteEvent(new FakeElement()));

      expect(handlerCalls).toBe(1);
    })
  );

  test('routes contentEditable=false to the focused handler', () =>
    withFakeBrowserGlobals(async () => {
      let handlerCalls = 0;
      PasteService.registerHandler('composer', async () => {
        handlerCalls += 1;
        return false;
      });
      PasteService.setLastFocusedComponent('composer');
      PasteService.init();

      await dispatchGlobalPaste(createPasteEvent(new FakeElement('DIV', { contentEditable: 'false' })));

      expect(handlerCalls).toBe(1);
    })
  );

  test('does not let an outer editable ancestor override contentEditable=false', () =>
    withFakeBrowserGlobals(async () => {
      let handlerCalls = 0;
      PasteService.registerHandler('composer', async () => {
        handlerCalls += 1;
        return false;
      });
      PasteService.setLastFocusedComponent('composer');
      PasteService.init();

      const outer = new FakeElement('DIV', { contentEditable: 'true' });
      const blocked = new FakeElement('DIV', { contentEditable: 'false' });
      const child = new FakeElement('SPAN', { editableAncestor: blocked });
      await dispatchGlobalPaste(createPasteEvent(child, [child, blocked, outer]));

      expect(handlerCalls).toBe(1);
    })
  );

  test('does not revive an unregistered focused handler if its id is reused', () =>
    withFakeBrowserGlobals(async () => {
      let handlerCalls = 0;
      PasteService.registerHandler('stale', async () => {
        handlerCalls += 1;
        return false;
      });
      PasteService.setLastFocusedComponent('stale');
      PasteService.unregisterHandler('stale');
      PasteService.registerHandler('stale', async () => {
        handlerCalls += 1;
        return false;
      });
      PasteService.init();

      await dispatchGlobalPaste(createPasteEvent(new FakeElement()));

      expect(handlerCalls).toBe(0);
    })
  );
});
