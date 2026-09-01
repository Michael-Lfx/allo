import { expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('./main.tsx', import.meta.url), 'utf8');
const appProvidersStart = source.indexOf('const AppProviders');
const appProvidersEnd = source.indexOf('\n\nconst Config', appProvidersStart);
const appProvidersSource = source.slice(appProvidersStart, appProvidersEnd);

test('keeps provider-owned modals under ThemeProvider', () => {
  expect(appProvidersStart).toBeGreaterThanOrEqual(0);
  expect(appProvidersEnd).toBeGreaterThan(appProvidersStart);
  expect(appProvidersSource).toMatch(
    /React\.createElement\(\s*ThemeProvider,\s*null,\s*React\.createElement\(\s*SupportChatProvider,\s*null/
  );
});
