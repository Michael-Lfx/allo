import { afterEach, describe, expect, test } from 'bun:test';

import {
  getBaseName,
  inferMimeFromExt,
  isPhysicalPointOverAnyDropzone,
  isPhysicalPointOverElement,
  normalizePath,
  pathsToFileMetadata,
  registerDropzone,
} from './tauriDragDrop';

describe('getBaseName', () => {
  test('POSIX path', () => {
    expect(getBaseName('/home/user/file.png')).toBe('file.png');
  });

  test('Windows path', () => {
    expect(getBaseName('C:\\Users\\Alice\\file.png')).toBe('file.png');
  });

  test('trailing separators are stripped', () => {
    expect(getBaseName('/home/user/dir/')).toBe('dir');
    expect(getBaseName('C:\\dir\\')).toBe('dir');
  });

  test('single segment falls back to itself', () => {
    expect(getBaseName('file.png')).toBe('file.png');
  });
});

describe('normalizePath', () => {
  test('trims whitespace', () => {
    expect(normalizePath('  /a/b.png  ')).toBe('/a/b.png');
  });

  test('strips one layer of double quotes', () => {
    expect(normalizePath('"/home/user/My File.png"')).toBe('/home/user/My File.png');
  });

  test('strips one layer of single quotes', () => {
    expect(normalizePath("'/home/user/My File.png'")).toBe('/home/user/My File.png');
  });

  test('decodes POSIX file:// URL', () => {
    expect(normalizePath('file:///tmp/example.png')).toBe('/tmp/example.png');
  });

  test('decodes percent-encoded file:// URL', () => {
    expect(normalizePath('file:///tmp/My%20File.png')).toBe('/tmp/My File.png');
  });

  test('decodes Windows file:// drive letter', () => {
    expect(normalizePath('file:///C:/Users/Alice/file.png')).toBe('C:/Users/Alice/file.png');
  });

  test('returns a plain absolute path unchanged', () => {
    expect(normalizePath('/home/user/file.png')).toBe('/home/user/file.png');
  });
});

describe('inferMimeFromExt', () => {
  test('known image types (case-insensitive)', () => {
    expect(inferMimeFromExt('/a/b/c.PNG')).toBe('image/png');
    expect(inferMimeFromExt('/a/b/c.jpg')).toBe('image/jpeg');
    expect(inferMimeFromExt('/a/b/c.jpeg')).toBe('image/jpeg');
    expect(inferMimeFromExt('/a/b/c.webp')).toBe('image/webp');
  });

  test('known document types', () => {
    expect(inferMimeFromExt('/a/b/c.pdf')).toBe('application/pdf');
  });

  test('unknown / missing extension returns empty string', () => {
    expect(inferMimeFromExt('/a/b/c.unknownext')).toBe('');
    expect(inferMimeFromExt('/a/b/noext')).toBe('');
  });
});

describe('pathsToFileMetadata', () => {
  test('builds metadata with path, basename and inferred type', () => {
    const result = pathsToFileMetadata(['/home/user/file.png', 'C:\\docs\\report.pdf']);
    expect(result).toHaveLength(2);
    expect(result[0]).toMatchObject({ path: '/home/user/file.png', name: 'file.png', type: 'image/png' });
    expect(result[1]).toMatchObject({ path: 'C:\\docs\\report.pdf', name: 'report.pdf', type: 'application/pdf' });
    expect(typeof result[0].lastModified).toBe('number');
    expect(result[0].size).toBe(0);
  });

  test('normalizes the stored path', () => {
    const result = pathsToFileMetadata(['  "/tmp/a.txt"  ']);
    expect(result[0].path).toBe('/tmp/a.txt');
    expect(result[0].name).toBe('a.txt');
  });
});

describe('isPhysicalPointOverElement', () => {
  const originalWindow = (globalThis as { window?: unknown }).window;
  const originalDocument = (globalThis as { document?: unknown }).document;

  // Tests run under bun (no DOM); install a minimal window/document per test.
  const setupDOM = (devicePixelRatio: number, elementsFromPoint: (x: number, y: number) => Element[]) => {
    Object.defineProperty(globalThis, 'window', { configurable: true, value: { devicePixelRatio } });
    Object.defineProperty(globalThis, 'document', { configurable: true, value: { elementsFromPoint } });
  };

  afterEach(() => {
    if (originalWindow === undefined) {
      Reflect.deleteProperty(globalThis, 'window');
    } else {
      Object.defineProperty(globalThis, 'window', { configurable: true, value: originalWindow });
    }
    if (originalDocument === undefined) {
      Reflect.deleteProperty(globalThis, 'document');
    } else {
      Object.defineProperty(globalThis, 'document', { configurable: true, value: originalDocument });
    }
  });

  test('returns false for null/undefined target', () => {
    setupDOM(1, () => []);
    expect(isPhysicalPointOverElement(10, 10, null)).toBe(false);
    expect(isPhysicalPointOverElement(10, 10, undefined)).toBe(false);
  });

  test('hits when target itself is in the element stack', () => {
    const target = { contains: () => false } as unknown as HTMLElement;
    const other = {} as Element;
    setupDOM(1, () => [other, target]);
    expect(isPhysicalPointOverElement(10, 10, target)).toBe(true);
  });

  test('hits when a descendant of target is in the stack (contains)', () => {
    const target = { contains: () => true } as unknown as HTMLElement;
    const child = {} as Element;
    setupDOM(1, () => [child]);
    expect(isPhysicalPointOverElement(10, 10, target)).toBe(true);
  });

  test('misses when the stack is disjoint from target', () => {
    const target = { contains: () => false } as unknown as HTMLElement;
    const other = {} as Element;
    setupDOM(1, () => [other]);
    expect(isPhysicalPointOverElement(10, 10, target)).toBe(false);
  });

  test('converts physical → CSS pixels via devicePixelRatio', () => {
    let received = { x: -1, y: -1 };
    const target = { contains: () => false } as unknown as HTMLElement;
    setupDOM(2, (x, y) => {
      received = { x, y };
      return [target];
    });
    // physical (20,20) ÷ DPR 2 = CSS (10,10)
    expect(isPhysicalPointOverElement(20, 20, target)).toBe(true);
    expect(received).toEqual({ x: 10, y: 10 });
  });
});

describe('dropzone registry', () => {
  const originalWindow = (globalThis as { window?: unknown }).window;
  const originalDocument = (globalThis as { document?: unknown }).document;
  const cleanups: Array<() => void> = [];

  // Registry is a module-level Set shared across tests; install a minimal DOM
  // (DPR 1) and drain any registered zones after each test so they don't leak.
  const setupDOM = (elementsFromPoint: (x: number, y: number) => Element[]) => {
    Object.defineProperty(globalThis, 'window', { configurable: true, value: { devicePixelRatio: 1 } });
    Object.defineProperty(globalThis, 'document', { configurable: true, value: { elementsFromPoint } });
  };

  afterEach(() => {
    while (cleanups.length) cleanups.pop()!();
    if (originalWindow === undefined) {
      Reflect.deleteProperty(globalThis, 'window');
    } else {
      Object.defineProperty(globalThis, 'window', { configurable: true, value: originalWindow });
    }
    if (originalDocument === undefined) {
      Reflect.deleteProperty(globalThis, 'document');
    } else {
      Object.defineProperty(globalThis, 'document', { configurable: true, value: originalDocument });
    }
  });

  test('returns false with no registered dropzones', () => {
    setupDOM(() => []);
    expect(isPhysicalPointOverAnyDropzone(5, 5)).toBe(false);
  });

  test('returns true once a covering dropzone is registered', () => {
    const target = { contains: () => true } as unknown as HTMLElement;
    setupDOM(() => [target]);
    expect(isPhysicalPointOverAnyDropzone(5, 5)).toBe(false);
    cleanups.push(registerDropzone(target));
    expect(isPhysicalPointOverAnyDropzone(5, 5)).toBe(true);
  });

  test('unregister removes the dropzone', () => {
    const target = { contains: () => true } as unknown as HTMLElement;
    setupDOM(() => [target]);
    const unregister = registerDropzone(target);
    expect(isPhysicalPointOverAnyDropzone(5, 5)).toBe(true);
    unregister();
    expect(isPhysicalPointOverAnyDropzone(5, 5)).toBe(false);
  });

  test('returns false when the registered dropzone does not cover the point', () => {
    const target = { contains: () => false } as unknown as HTMLElement;
    const other = {} as Element;
    setupDOM(() => [other]);
    cleanups.push(registerDropzone(target));
    expect(isPhysicalPointOverAnyDropzone(5, 5)).toBe(false);
  });
});
