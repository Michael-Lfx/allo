/**
 * Tests for the free-ports dev-loop classifier. Importing the module must not
 * run the port-killing preflight (guarded by `import.meta.main`).
 */
import { describe, expect, test } from 'bun:test';

import { looksLikeDevLoop } from './free-ports.mjs';

describe('looksLikeDevLoop', () => {
  test('matches the dev-loop process shapes free-ports actually kills', () => {
    expect(
      looksLikeDevLoop('node "D:\\repo\\ui\\node_modules\\vite\\bin\\vite.js" --mode webdev'),
    ).toBe(true);
    expect(looksLikeDevLoop('target\\debug\\nomifun-web.exe --port 8787 --api-only')).toBe(true);
    expect(looksLikeDevLoop('cargo run -p nomifun-web -- --port 8787')).toBe(true);
    expect(looksLikeDevLoop('node tauri.js dev --config apps/desktop/tauri.conf.json')).toBe(true);
    expect(looksLikeDevLoop('concurrently -k -n api,ui "cargo run" "bun run dev:web"')).toBe(true);
    expect(looksLikeDevLoop('agent-store --port 8787')).toBe(true);
  });

  test('does not match unrelated port holders', () => {
    expect(looksLikeDevLoop('')).toBe(false);
    expect(looksLikeDevLoop('C:\\Program Files\\MyApp\\service.exe --listen 8787')).toBe(false);
    expect(looksLikeDevLoop('python -m http.server 5173')).toBe(false);
  });
});
