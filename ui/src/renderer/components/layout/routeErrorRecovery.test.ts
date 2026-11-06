import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

import {
  claimDynamicImportReload,
  DYNAMIC_IMPORT_RETRY_STORAGE_KEY,
  DYNAMIC_IMPORT_RETRY_WINDOW_MS,
  isDomCommitFailure,
  isDynamicImportFailure,
} from './routeErrorRecovery';

function createStorage() {
  const values = new Map<string, string>();
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
  };
}

describe('dynamic import route recovery', () => {
  test('recognizes browser module-load failures but not ordinary route errors', () => {
    expect(isDynamicImportFailure(new TypeError('Failed to fetch dynamically imported module'))).toBe(true);
    expect(isDynamicImportFailure(new Error('Importing a module script failed'))).toBe(true);
    expect(
      isDynamicImportFailure(
        new SyntaxError(
          "The requested module '/src/renderer/pages/guid/components/GuidEntryFlyout.tsx' does not provide an export named 'entryFlyoutAlignTop'",
        ),
      ),
    ).toBe(true);
    expect(isDynamicImportFailure(new Error('route component crashed'))).toBe(false);
  });

  test('allows one reload per URL within the retry window', () => {
    const storage = createStorage();
    const href = 'http://127.0.0.1:5173/#/guid';

    expect(claimDynamicImportReload(storage, href, 1_000)).toBe(true);
    expect(storage.getItem(DYNAMIC_IMPORT_RETRY_STORAGE_KEY)).toContain(href);
    expect(claimDynamicImportReload(storage, href, 1_001)).toBe(false);
    expect(claimDynamicImportReload(storage, href, 1_000 + DYNAMIC_IMPORT_RETRY_WINDOW_MS + 1)).toBe(true);
  });

  test('fails closed when storage is unavailable', () => {
    expect(claimDynamicImportReload(undefined, 'http://localhost/#/guid', 100)).toBe(false);
    expect(claimDynamicImportReload(null, 'http://localhost/#/guid', 100)).toBe(false);
  });

  test('fails closed when storage cannot be read or written', () => {
    const readFailure = {
      getItem: () => {
        throw new Error('storage blocked');
      },
      setItem: () => undefined,
    };
    const writeFailure = {
      getItem: () => null,
      setItem: () => {
        throw new Error('storage blocked');
      },
    };

    expect(claimDynamicImportReload(readFailure, 'http://localhost/#/guid', 100)).toBe(false);
    expect(claimDynamicImportReload(writeFailure, 'http://localhost/#/guid', 100)).toBe(false);
  });

  test('fails closed when the retry record is malformed', () => {
    const storage = createStorage();
    storage.setItem(DYNAMIC_IMPORT_RETRY_STORAGE_KEY, '{not-json');

    expect(claimDynamicImportReload(storage, 'http://localhost/#/guid', 100)).toBe(false);
  });

  test('uses the same bounded recovery gate in the nested settings boundary', () => {
    const source = readFileSync(new URL('./SettingsSiderErrorBoundary.tsx', import.meta.url), 'utf8');
    expect(source).toContain('claimDynamicImportReload');
    expect(source).toContain('dynamicImportReloadExhausted');
  });

  test('makes the exhausted dynamic-import action explicit', () => {
    const source = readFileSync(new URL('./RouteErrorBoundary.tsx', import.meta.url), 'utf8');
    expect(source).toContain("? 'reloadAgain'");
    expect(source).toContain("? 'reloadApplication'");
    expect(source).toContain('useTranslation');
    expect(source).toContain('var(--flowy-panel');
  });

  test('updates the settings action after the bounded reload claim is exhausted', () => {
    const source = readFileSync(new URL('./SettingsSiderErrorBoundary.tsx', import.meta.url), 'utf8');
    expect(source).toContain('reloadExhausted: boolean');
    expect(source).toContain('reloadExhausted={dynamicImportReloadExhausted}');
    expect(source).toContain('common.routeError.reloadAgain');
  });
});

describe('dom commit route recovery', () => {
  const domException = (name: string, message: string) => Object.assign(new Error(message), { name });

  test('recognizes a detached-node commit failure thrown by React', () => {
    expect(
      isDomCommitFailure(
        domException(
          'NotFoundError',
          "Failed to execute 'removeChild' on 'Node': The node to be removed is not a child of this node.",
        ),
      ),
    ).toBe(true);
    expect(
      isDomCommitFailure(
        domException(
          'NotFoundError',
          "Failed to execute 'insertBefore' on 'Node': The node before which the new node is to be inserted is not a child of this node.",
        ),
      ),
    ).toBe(true);
    expect(
      isDomCommitFailure(
        domException('HierarchyRequestError', 'Failed to execute appendChild: The new child element contains the parent.'),
      ),
    ).toBe(true);
  });

  test('classifies by DOMException name even when the message text is localized', () => {
    expect(isDomCommitFailure(domException('NotFoundError', '无法在“Node”上执行“removeChild”'))).toBe(true);
  });

  test('keeps a message-only fallback for engines that do not forward the DOMException name', () => {
    expect(
      isDomCommitFailure(new Error("Failed to execute 'removeChild' on 'Node': The node to be removed is not a child of this node.")),
    ).toBe(true);
  });

  test('does not classify unrelated route errors as DOM commit failures', () => {
    expect(isDomCommitFailure(new Error('route component crashed'))).toBe(false);
    expect(isDomCommitFailure(new Error('removeChild failed inside a test helper'))).toBe(false);
    expect(isDomCommitFailure(domException('TypeError', 'x is not a function'))).toBe(false);
    expect(isDomCommitFailure(null)).toBe(false);
    expect(isDomCommitFailure(undefined)).toBe(false);
  });

  test('wires one-shot remount recovery into route and settings-rail boundaries', () => {
    const boundarySource = readFileSync(new URL('./RouteErrorBoundary.tsx', import.meta.url), 'utf8');
    expect(boundarySource).toContain('DomCommitRecovery');
    expect(boundarySource).toContain("scope !== 'application'");
    const siderBoundarySource = readFileSync(new URL('./SettingsSiderErrorBoundary.tsx', import.meta.url), 'utf8');
    expect(siderBoundarySource).toContain('DomCommitRecovery');
    const siderSource = readFileSync(new URL('./Sider/index.tsx', import.meta.url), 'utf8');
    expect(siderSource).toContain('location.key');
  });

  test('every navigation, including a same-route new-task click, resets a captured route error', () => {
    const source = readFileSync(new URL('./Router.tsx', import.meta.url), 'utf8');
    expect(source).toContain('`${location.key}:${location.pathname}${location.search}${location.hash}`');
  });
});
