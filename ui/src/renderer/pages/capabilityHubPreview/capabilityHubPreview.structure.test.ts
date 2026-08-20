import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';
import { HUB_IDS } from './fixtures';

const pageSource = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');
const mockSource = readFileSync(new URL('./HubMock.tsx', import.meta.url), 'utf8');
const routerSource = readFileSync(new URL('../../components/layout/Router.tsx', import.meta.url), 'utf8');

describe('capability hub preview', () => {
  test('registers the hidden test route outside ProtectedLayout', () => {
    expect(routerSource.includes("path='/test/capability-hub'")).toBe(true);
    expect(routerSource.includes("import('@renderer/pages/capabilityHubPreview')")).toBe(true);

    const previewIndex = routerSource.indexOf("path='/test/capability-hub'");
    const protectedIndex = routerSource.indexOf('element={<ProtectedLayout');
    expect(previewIndex).toBeGreaterThan(-1);
    expect(protectedIndex).toBeGreaterThan(-1);
    expect(previewIndex).toBeLessThan(protectedIndex);
  });

  test('mocks the four hubs and a now versus proposed switch', () => {
    expect(HUB_IDS).toEqual(['presets', 'skills', 'mcp', 'plugins']);
    expect(pageSource.includes("useState<HubPreviewVariant>('proposed')")).toBe(true);
    expect(mockSource.includes('data-variant={variant}')).toBe(true);
    expect(mockSource.includes("capabilityHubPreview.discover")).toBe(true);
  });

  test('owns the locked app viewport as its own scrollport', () => {
    const css = readFileSync(new URL('./capabilityHubPreview.module.css', import.meta.url), 'utf8');
    expect(css.includes('position: fixed')).toBe(true);
    expect(css.includes('overflow-y: auto')).toBe(true);
  });
});
