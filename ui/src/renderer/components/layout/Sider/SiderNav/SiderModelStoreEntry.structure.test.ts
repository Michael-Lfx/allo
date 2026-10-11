import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const siderSource = readFileSync(new URL('../index.tsx', import.meta.url), 'utf8');
const entrySource = readFileSync(new URL('./SiderModelStoreEntry.tsx', import.meta.url), 'utf8');

describe('Sider model store entry', () => {
  test('sits directly below the search entry in the primary nav', () => {
    const searchIndex = siderSource.indexOf('<SiderSearchEntry');
    const storeIndex = siderSource.indexOf('<SiderModelStoreEntry');
    expect(searchIndex).toBeGreaterThan(-1);
    expect(storeIndex).toBeGreaterThan(searchIndex);
  });

  test('navigates to /model-store', () => {
    expect(siderSource.includes("navTo('/model-store')")).toBe(true);
    expect(siderSource.includes("pathname.startsWith('/model-store')")).toBe(true);
  });

  test('routes /model-store to the model store page', () => {
    const routerSource = readFileSync(new URL('../../Router.tsx', import.meta.url), 'utf8');
    expect(routerSource.includes("import('@renderer/pages/modelStore')")).toBe(true);
    expect(routerSource.includes("path='/model-store'")).toBe(true);
  });

  test('mirrors the search button chrome and active state', () => {
    expect(entrySource.includes('h-34px')).toBe(true);
    expect(entrySource.includes('border-none')).toBe(true);
    expect(entrySource.includes('border-[var(--color-border-2)]')).toBe(false);
    expect(entrySource.includes('bg-primary-1')).toBe(true);
    expect(entrySource.includes('text-primary-6')).toBe(true);
  });

  test('shows the Ctrl+M chrome shortcut badge and tooltip', () => {
    expect(entrySource.includes("isAppChromeShortcutAvailable('modelStore'")).toBe(true);
    expect(entrySource.includes("formatAppChromeShortcut('modelStore')")).toBe(true);
    expect(entrySource.includes("appChromeShortcutTooltip(label, 'modelStore')")).toBe(true);
    expect(entrySource.includes('APP_CHROME_SHORTCUT_BADGE_CLASS')).toBe(true);
  });
});
