/**
 * P1 fluency probe — Windows Edge headless (no Playwright dependency).
 *
 * Usage:
 *   bun scripts/ux-fluency-probe.mjs --url http://127.0.0.1:5173
 *
 * Writes under docs/superpowers/audits/artifacts/p1-<timestamp>/
 * and merges summary into docs/superpowers/audits/p1-fluency-baselines.json
 */

import { mkdirSync, writeFileSync, existsSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';
import { tmpdir } from 'node:os';

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = join(__dirname, '..');
const stamp = new Date().toISOString().replace(/[:.]/g, '-');
const outDir = join(root, 'docs/superpowers/audits/artifacts', `p1-${stamp}`);
mkdirSync(outDir, { recursive: true });

const urlArg =
  process.argv.find((a) => a.startsWith('--url='))?.slice(6) ??
  (process.argv.includes('--url') ? process.argv[process.argv.indexOf('--url') + 1] : null) ??
  'http://127.0.0.1:5173';

const EDGE_CANDIDATES = [
  'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
  'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
];

const edgePath = EDGE_CANDIDATES.find((p) => existsSync(p));
if (!edgePath) {
  console.error('Microsoft Edge not found');
  process.exit(1);
}

const viewports = [
  { name: '1280x720', width: 1280, height: 720 },
  { name: '1440x900', width: 1440, height: 900 },
  { name: '2560x1440', width: 2560, height: 1440 },
];

const themes = ['light', 'dark'];

const BASELINE_TARGETS = {
  coldStartMs: { p50: 2500, p95: 4500 },
  routeSwitchMs: { p50: 180, p95: 450 },
  inputResponseMs: { p50: 50, p95: 120 },
  longTasksPerMinute: { p50: 8, p95: 20 },
  cls: { p50: 0.05, p95: 0.1 },
  animationFps: { p50: 50, p95: 45 },
};

function run(cmd, args, opts = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(cmd, args, { windowsHide: true, ...opts });
    let stdout = '';
    let stderr = '';
    child.stdout?.on('data', (d) => {
      stdout += d.toString();
    });
    child.stderr?.on('data', (d) => {
      stderr += d.toString();
    });
    child.on('error', reject);
    child.on('close', (code) => resolve({ code, stdout, stderr }));
  });
}

async function fetchTiming(url) {
  const t0 = Date.now();
  const res = await fetch(url, { redirect: 'follow' });
  const text = await res.text();
  return {
    httpMs: Date.now() - t0,
    status: res.status,
    bytes: text.length,
    hasRoot: text.includes('id="root"') || text.includes("id='root'"),
  };
}

async function edgeScreenshot({ url, width, height, outPng }) {
  const userData = join(tmpdir(), `flowy-p1-edge-${Date.now()}-${Math.random().toString(16).slice(2)}`);
  mkdirSync(userData, { recursive: true });
  const args = [
    '--headless=new',
    '--disable-gpu',
    '--no-first-run',
    '--no-default-browser-check',
    '--force-prefers-reduced-motion',
    '--virtual-time-budget=20000',
    `--user-data-dir=${userData}`,
    `--window-size=${width},${height}`,
    `--screenshot=${outPng}`,
    url,
  ];
  const t0 = Date.now();
  const result = await run(edgePath, args);
  // Edge may print "bytes written" after the process is still flushing; wait for file.
  const deadline = Date.now() + 15000;
  while (!existsSync(outPng) && Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 100));
  }
  if (existsSync(outPng)) {
    let last = -1;
    for (let i = 0; i < 20; i++) {
      const { size } = statSync(outPng);
      if (size > 0 && size === last) break;
      last = size;
      await new Promise((r) => setTimeout(r, 100));
    }
  }
  return { ms: Date.now() - t0, ...result, exists: existsSync(outPng) };
}

const findings = [];
const screenshots = [];
const coldStartMs = [];
const routeSamples = [];

const boot = await fetchTiming(urlArg);
if (boot.status !== 200 || !boot.hasRoot) {
  findings.push({
    severity: 'P0',
    area: 'boot',
    message: `UI root not healthy: status=${boot.status} hasRoot=${boot.hasRoot}`,
  });
}

for (const vp of viewports) {
  for (const theme of themes) {
    const hashUrl = `${urlArg}/#/login`;
    const shot = join(outDir, `${vp.name}-${theme}.png`);
    const result = await edgeScreenshot({
      url: hashUrl,
      width: vp.width,
      height: vp.height,
      outPng: shot,
    });
    coldStartMs.push(result.ms);
    if (!result.exists) {
      findings.push({
        severity: 'P1',
        area: 'screenshot',
        message: `screenshot missing for ${vp.name}/${theme}`,
        stderr: result.stderr?.slice(0, 400),
      });
    } else {
      screenshots.push(shot);
    }
    writeFileSync(
      join(outDir, `${vp.name}-${theme}.json`),
      JSON.stringify(
        {
          viewport: vp,
          theme,
          url: hashUrl,
          captureMs: result.ms,
          exitCode: result.code,
          note: 'Theme toggle is app-controlled; Edge capture is initial paint of /#/login.',
        },
        null,
        2
      )
    );
  }
}

for (const hash of ['#/login', '#/cloud-login', '#/guid']) {
  const t0 = Date.now();
  const timing = await fetchTiming(`${urlArg}/${hash}`);
  routeSamples.push({ hash, ms: Date.now() - t0, ...timing });
}
routeSamples.sort((a, b) => a.ms - b.ms);
const routeP50 = routeSamples[Math.floor(routeSamples.length * 0.5)]?.ms ?? null;
const routeP95 = routeSamples[Math.min(routeSamples.length - 1, Math.floor(routeSamples.length * 0.95))]?.ms ?? null;

coldStartMs.sort((a, b) => a - b);
const coldP50 = coldStartMs[Math.floor(coldStartMs.length * 0.5)] ?? null;
const coldP95 = coldStartMs[Math.min(coldStartMs.length - 1, Math.floor(coldStartMs.length * 0.95))] ?? null;

const summary = {
  generatedAt: new Date().toISOString(),
  url: urlArg,
  artifactDir: outDir.replace(/\\/g, '/'),
  targets: BASELINE_TARGETS,
  measured: {
    httpBoot: boot,
    coldStartMs: { samples: coldStartMs, p50: coldP50, p95: coldP95, method: 'edge-headless-screenshot' },
    routeSwitchMs: {
      samples: routeSamples,
      p50: routeP50,
      p95: routeP95,
      method: 'http-fetch-hash-url (document shell; SPA paint measured in authenticated retest)',
    },
    inputResponseMs: null,
    longTasksPerMinute: null,
    cls: null,
    animationFps: null,
    note: 'Authenticated path metrics (input / CLS / FPS / long tasks) require logged-in desktop session; record on full-path retest.',
  },
  codeAuditFixes: [
    'ProtectedLayout keeps shell + AppLoader fill',
    'ConversationShell eager import',
    'workspace collapse sync localStorage',
    'session sider keep-mounted when collapsed',
    'guid max-height 760 padding',
    'popover unmountOnExit=false',
  ],
  findings,
  screenshots: screenshots.map((p) => p.replace(/\\/g, '/')),
  visualNotes: [
    'Login shell uses framer-motion initial opacity:0; without reduced-motion, first paint can look blank (flash).',
    'Login card visual theme is mostly self-contained; data-theme dark does not fully recolor the login split panel in headless capture.',
    'Authenticated guid/workspace CLS/FPS not measured in this run (no credentials on probe machine).',
  ],
};

writeFileSync(join(outDir, 'summary.json'), JSON.stringify(summary, null, 2));
writeFileSync(join(root, 'docs/superpowers/audits/p1-fluency-baselines.json'), JSON.stringify(summary, null, 2));

console.log(
  JSON.stringify(
    {
      ok: findings.every((f) => f.severity !== 'P0'),
      outDir: outDir.replace(/\\/g, '/'),
      coldP50,
      coldP95,
      routeP50,
      routeP95,
      screenshots: screenshots.length,
      findingCount: findings.length,
    },
    null,
    2
  )
);
