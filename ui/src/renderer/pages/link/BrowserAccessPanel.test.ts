import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const panelSource = readFileSync(new URL('./BrowserAccessPanel.tsx', import.meta.url), 'utf8');
const pageSource = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');

describe('browser scan on the connect-phone page', () => {
  test('puts an http WebUI login QR ahead of the app pairing code', () => {
    expect(panelSource.includes('buildWebuiQrLoginUrl')).toBe(true);
    expect(panelSource.includes('listBrowserScanBaseUrls')).toBe(true);
    expect(panelSource.includes('flowy://')).toBe(false);
    expect(pageSource.indexOf('<BrowserAccessPanel />')).toBeLessThan(pageSource.indexOf('<LinkPairingPanel />'));
  });
});
