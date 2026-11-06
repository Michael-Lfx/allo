#!/usr/bin/env bun
/**
 * Renders the home "choose a style" covers for presentation themes.
 *
 *   bun scripts/slide-theme-covers/render.mjs            all themes
 *   bun scripts/slide-theme-covers/render.mjs swiss navy  selected themes
 *
 * Each `themes/<id>.html` is a 1280x720 cover authored with the theme's
 * palette (ui/src/renderer/pages/guid/presentation/slideThemes.ts) and its
 * ppt-master visual style. Headless Edge/Chrome renders it over CDP, waits for
 * web fonts (Google Fonts, OFL; only rendered into the image, never bundled),
 * and writes a 640x360 WebP next to the thumbnail component.
 * Set CHROME_PATH to override the browser.
 */
import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const THEMES = join(HERE, 'themes');
const OUT = join(HERE, '..', '..', 'ui', 'src', 'renderer', 'pages', 'guid', 'presentation', 'covers');
const W = 1280;
const H = 720;

const BROWSERS = [
  process.env.CHROME_PATH,
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  'C:/Program Files/Microsoft/Edge/Application/msedge.exe',
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
  '/usr/bin/google-chrome',
  '/usr/bin/chromium',
].filter(Boolean);

const browser = BROWSERS.find((path) => existsSync(path));
if (!browser) {
  console.error('No Edge/Chrome found; set CHROME_PATH.');
  process.exit(1);
}

const all = readdirSync(THEMES)
  .filter((name) => name.endsWith('.html'))
  .map((name) => name.slice(0, -'.html'.length));
const ids = process.argv.slice(2).length ? process.argv.slice(2) : all;
const unknown = ids.filter((id) => !all.includes(id));
if (unknown.length) {
  console.error(`Unknown theme(s): ${unknown.join(', ')}`);
  process.exit(1);
}

const profile = mkdtempSync(join(tmpdir(), 'slide-covers-'));
const proc = spawn(browser, [
  '--headless=new',
  '--remote-debugging-port=0',
  `--user-data-dir=${profile}`,
  '--hide-scrollbars',
  '--no-first-run',
  '--no-default-browser-check',
  '--allow-file-access-from-files',
  'about:blank',
]);

const wsUrl = await new Promise((resolve, reject) => {
  let buffer = '';
  const timer = setTimeout(() => reject(new Error('browser did not start')), 20000);
  proc.stderr.on('data', (chunk) => {
    buffer += chunk;
    const match = buffer.match(/DevTools listening on (ws:\/\/\S+)/);
    if (match) {
      clearTimeout(timer);
      resolve(match[1]);
    }
  });
});

const ws = new WebSocket(wsUrl);
await new Promise((resolve) => ws.addEventListener('open', resolve, { once: true }));
let nextId = 0;
const pending = new Map();
ws.addEventListener('message', (event) => {
  const message = JSON.parse(event.data);
  const waiter = pending.get(message.id);
  if (!waiter) return;
  pending.delete(message.id);
  if (message.error) waiter.reject(new Error(message.error.message));
  else waiter.resolve(message.result);
});
const send = (method, params = {}, sessionId) =>
  new Promise((resolve, reject) => {
    const id = ++nextId;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params, sessionId }));
  });

const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
const page = (method, params) => send(method, params, sessionId);
await page('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: 1, mobile: false });

mkdirSync(OUT, { recursive: true });
try {
  for (const id of ids) {
    await page('Page.navigate', { url: pathToFileURL(join(THEMES, `${id}.html`)).href });
    const { result } = await page('Runtime.evaluate', {
      expression: `(async () => {
        const deadline = Date.now() + 15000;
        while (document.readyState !== 'complete' && Date.now() < deadline) await new Promise((r) => setTimeout(r, 50));
        await document.fonts.ready;
        await new Promise((r) => setTimeout(r, 300));
        return [...document.fonts].filter((f) => f.status === 'error').map((f) => f.family).join(', ');
      })()`,
      awaitPromise: true,
      returnByValue: true,
    });
    if (result.value) console.warn(`${id}: fonts failed to load: ${result.value}`);
    const { data } = await page('Page.captureScreenshot', {
      format: 'webp',
      quality: 88,
      clip: { x: 0, y: 0, width: W, height: H, scale: 0.5 },
    });
    const bytes = Buffer.from(data, 'base64');
    writeFileSync(join(OUT, `${id}.webp`), bytes);
    console.log(`${id}.webp  ${(bytes.length / 1024).toFixed(1)} KB`);
  }
} finally {
  ws.close();
  proc.kill();
  await new Promise((r) => setTimeout(r, 500));
  rmSync(profile, { recursive: true, force: true, maxRetries: 5 });
}
