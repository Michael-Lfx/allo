#!/usr/bin/env node
/**
 * Bounded Edge DOM matrix for the feedback and support-chat modal surfaces.
 *
 * Usage:
 *   bun run check:support-surface -- --url http://127.0.0.1:5173 --smoke
 *   bun run check:support-surface -- --url http://127.0.0.1:5173 --full --max-runs 128
 *
 * This is intentionally separate from source contracts and manual visual
 * acceptance. Each run uses a loopback URL, an isolated profile and confirmed
 * process/profile cleanup.
 */

import { existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const stamp = new Date().toISOString().replace(/[:.]/g, '-');
const outDir = join(root, '.superpowers', 'audits', 'artifacts', `support-surface-${stamp}`);
mkdirSync(outDir, { recursive: true });

const args = process.argv.slice(2);
const readArg = (name, fallback) => {
  const index = args.indexOf(name);
  return index === -1 ? fallback : args[index + 1] ?? fallback;
};
const hasArg = (name) => args.includes(name);
const urlArg = readArg('--url', 'http://127.0.0.1:5173');
const smoke = hasArg('--smoke');
const fullMatrix = hasArg('--full');
const screenshots = hasArg('--screenshots');
const maxCasesPerRun = 128;
const maxAttemptsPerCase = 3;
const requestedMaxRuns = Number(readArg('--max-runs', ''));
if (hasArg('--max-runs') && (!Number.isInteger(requestedMaxRuns) || requestedMaxRuns < 1)) {
  throw new Error(`[check:support-surface] --max-runs must be an integer between 1 and ${maxCasesPerRun}`);
}
if (hasArg('--max-runs') && requestedMaxRuns > maxCasesPerRun) {
  throw new Error(`[check:support-surface] --max-runs cannot exceed ${maxCasesPerRun}`);
}
const maxRuns = hasArg('--max-runs')
  ? requestedMaxRuns
  : fullMatrix
    ? maxCasesPerRun
    : smoke
      ? 24
      : 0;
const edgeTimeoutMs = Math.max(1_000, Number(readArg('--timeout-ms', '30000')) || 30_000);
const edgeKillGraceMs = 5_000;

const allowedHosts = new Set(['localhost', '127.0.0.1', '[::1]']);
const baseUrl = new URL(urlArg);
if (!['http:', 'https:'].includes(baseUrl.protocol) || baseUrl.username || baseUrl.password) {
  throw new Error('[check:support-surface] --url must be an http(s) URL without credentials');
}
if (!allowedHosts.has(baseUrl.hostname.toLowerCase())) {
  throw new Error('[check:support-surface] --url is restricted to localhost, loopback IPv4, or loopback IPv6');
}

const edgeCandidates = [
  'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
  'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
];
const edgePath = edgeCandidates.find((candidate) => existsSync(candidate));
if (!edgePath) {
  console.error('[check:support-surface] Microsoft Edge not found');
  process.exit(1);
}

const matrix = {
  widths: smoke ? [360, 560, 1280] : [320, 360, 390, 480, 560, 768, 1280],
  heights: smoke ? [900] : [720, 900],
  dprs: smoke ? [1, 2] : [1, 1.25, 1.5, 2],
  locales: smoke ? ['zh-CN'] : ['zh-CN', 'en-US'],
  schemes: smoke ? ['light', 'dark'] : ['light', 'dark'],
  themes: smoke ? ['codex-neutral', 'rhythm-dark'] : [
    'codex-neutral',
    'rhythm-dark',
    'neon-rainbow',
    'frosted-glass',
    'sunset-afterglow',
    'notion',
  ],
  surfaces: ['feedback', 'support'],
  scenarios: smoke ? ['long', 'normal', 'log-confirm'] : [
    'normal',
    'empty',
    'long',
    'expanded',
    'screenshots',
    'sending',
    'uploading',
    'failed',
    'log-confirm',
  ],
};

const activeChildren = new Set();
let interrupted = false;

const terminateProcessTree = (child) => {
  if (!child?.pid) return Promise.resolve(true);
  if (process.platform === 'win32') {
    return new Promise((resolve) => {
      let settled = false;
      const finish = (success) => {
        if (settled) return;
        settled = true;
        clearTimeout(timeoutId);
        resolve(success);
      };
      const killer = spawn('taskkill', ['/PID', String(child.pid), '/T', '/F'], {
        stdio: 'ignore',
        windowsHide: true,
      });
      const timeoutId = setTimeout(() => {
        try {
          child.kill();
        } catch {
          // The process may already be gone; profile removal is checked below.
        }
        finish(false);
      }, edgeKillGraceMs);
      killer.on('error', () => {
        try {
          child.kill();
        } catch {
          // Ignore an already-closed child.
        }
        finish(false);
      });
      killer.on('close', (code) => finish(code === 0 || child.exitCode !== null || child.signalCode !== null));
    });
  }
  try {
    child.kill('SIGKILL');
  } catch {
    // Ignore an already-closed child.
  }
  return Promise.resolve(true);
};

const cleanupActiveChildren = async () => {
  const children = [...activeChildren];
  await Promise.all(children.map((child) => terminateProcessTree(child)));
  for (const child of children) activeChildren.delete(child);
};

const onInterrupt = () => {
  interrupted = true;
  void cleanupActiveChildren();
};
process.once('SIGINT', onInterrupt);
process.once('SIGTERM', onInterrupt);

const run = (command, commandArgs, { timeoutMs = edgeTimeoutMs, ...spawnOptions } = {}) =>
  new Promise((resolve) => {
    const child = spawn(command, commandArgs, { windowsHide: true, ...spawnOptions });
    activeChildren.add(child);
    let stdout = '';
    let stderr = '';
    let settled = false;
    let timedOut = false;
    let timeoutId;
    let killGraceId;

    const finish = ({ code = null, signal = null, spawnError = null } = {}) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeoutId);
      clearTimeout(killGraceId);
      resolve({ child, code, signal, stdout, stderr, timedOut, spawnError });
    };

    child.stdout?.on('data', (chunk) => {
      stdout += chunk.toString();
    });
    child.stderr?.on('data', (chunk) => {
      stderr += chunk.toString();
    });
    child.on('error', (error) => {
      stderr += `\n${error.message}`;
      finish({ spawnError: error.message });
    });
    child.on('close', (code, signal) => finish({ code, signal }));

    timeoutId = setTimeout(() => {
      timedOut = true;
      void terminateProcessTree(child);
      killGraceId = setTimeout(() => finish({ signal: 'SIGKILL' }), edgeKillGraceMs);
    }, timeoutMs);
  });

const buildUrl = (query) => {
  const url = new URL(urlArg);
  url.search = new URLSearchParams(query).toString();
  url.hash = '/test/support-surface';
  return url.toString();
};

const decodeHtml = (value) =>
  value
    .replaceAll('&quot;', '"')
    .replaceAll('&#39;', "'")
    .replaceAll('&lt;', '<')
    .replaceAll('&gt;', '>')
    .replaceAll('&amp;', '&');

const parseReport = (dom) => {
  const match = dom.match(/<pre id="support-surface-probe-result"[^>]*data-ready="true"[^>]*>([\s\S]*?)<\/pre>/i);
  if (!match) return null;
  try {
    return JSON.parse(decodeHtml(match[1].trim()));
  } catch {
    return null;
  }
};

const captureOnce = async ({ width, height, dpr, locale, scheme, theme, surface, scenario, index, attempt }) => {
  const name = [index, width, height, dpr, locale, scheme, theme, surface, scenario].join('-');
  const userDataDir = join(tmpdir(), `flowy-support-surface-${Date.now()}-${Math.random().toString(16).slice(2)}`);
  const dumpPath = join(outDir, `${name}${attempt ? `-retry-${attempt}` : ''}.html`);
  const screenshotPath = join(outDir, `${name}.png`);
  const url = buildUrl({ locale, scheme, theme, surface, scenario });
  const edgeArgs = [
    '--headless',
    '--disable-gpu',
    '--disable-gpu-compositing',
    '--disable-features=VizDisplayCompositor',
    '--in-process-gpu',
    '--no-first-run',
    '--no-default-browser-check',
    '--force-prefers-reduced-motion',
    '--virtual-time-budget=5000',
    `--force-device-scale-factor=${dpr}`,
    `--user-data-dir=${userDataDir}`,
    `--window-size=${width},${height}`,
    '--dump-dom',
    ...(screenshots ? [`--screenshot=${screenshotPath}`] : []),
    url,
  ];
  let result;
  try {
    result = await run(edgePath, edgeArgs);
    writeFileSync(dumpPath, result.stdout);
    return { name, url, result, report: parseReport(result.stdout), dumpPath, screenshotPath };
  } finally {
    if (result?.child) {
      await terminateProcessTree(result.child);
      activeChildren.delete(result.child);
    }
    let cleaned = false;
    for (let cleanupAttempt = 0; cleanupAttempt < 12; cleanupAttempt += 1) {
      try {
        rmSync(userDataDir, { recursive: true, force: true, maxRetries: 2, retryDelay: 100 });
        cleaned = true;
        break;
      } catch (error) {
        if (error?.code !== 'EBUSY') throw error;
        if (cleanupAttempt === 11) break;
        await new Promise((resolve) => setTimeout(resolve, 250 * (cleanupAttempt + 1)));
      }
    }
    if (!cleaned) throw new Error(`[check:support-surface] Edge profile cleanup was not confirmed: ${userDataDir}`);
  }
};

const capture = async (testCase) => {
  let captured;
  for (let attempt = 0; attempt < maxAttemptsPerCase; attempt += 1) {
    captured = await captureOnce({ ...testCase, attempt });
    if (captured.report) return captured;
    await new Promise((resolve) => setTimeout(resolve, 250 * (attempt + 1)));
  }
  return captured;
};

const caseKey = (testCase) =>
  [testCase.width, testCase.height, testCase.dpr, testCase.locale, testCase.scheme, testCase.theme, testCase.surface, testCase.scenario].join('|');

const representativeCases = () => {
  const result = [];
  const seen = new Set();
  const add = (values) => {
    const key = caseKey(values);
    if (seen.has(key)) return;
    seen.add(key);
    result.push({ ...values, index: result.length });
  };
  const base = {
    height: matrix.heights.at(-1),
    dpr: matrix.dprs[0],
    locale: matrix.locales[0],
    scheme: matrix.schemes[0],
    theme: matrix.themes[0],
    scenario: matrix.scenarios[0],
  };
  for (const surface of matrix.surfaces) {
    for (const width of matrix.widths) add({ ...base, width, surface });
    for (const height of matrix.heights) add({ ...base, width: matrix.widths[1], height, surface });
    for (const dpr of matrix.dprs) add({ ...base, width: matrix.widths[1], dpr, surface });
    for (const locale of matrix.locales) add({ ...base, width: matrix.widths[1], locale, surface });
    for (const scheme of matrix.schemes) add({ ...base, width: matrix.widths[1], scheme, surface });
    for (const theme of matrix.themes) add({ ...base, width: matrix.widths[1], theme, surface });
    for (const scenario of matrix.scenarios) add({ ...base, width: matrix.widths[1], scenario, surface });
  }
  return result;
};

const cartesianCases = () => {
  const result = [];
  for (const width of matrix.widths) {
    for (const height of matrix.heights) {
      for (const dpr of matrix.dprs) {
        for (const locale of matrix.locales) {
          for (const scheme of matrix.schemes) {
            for (const theme of matrix.themes) {
              for (const surface of matrix.surfaces) {
                for (const scenario of matrix.scenarios) {
                  result.push({ width, height, dpr, locale, scheme, theme, surface, scenario, index: result.length });
                }
              }
            }
          }
        }
      }
    }
  }
  return result;
};

const cases = fullMatrix ? cartesianCases() : representativeCases();
const selectedCases = cases.slice(0, Math.min(maxRuns || cases.length, maxCasesPerRun));
const failures = [];
const reports = [];

try {
  for (const testCase of selectedCases) {
    if (interrupted) throw new Error('[check:support-surface] interrupted; active Edge processes were terminated');
    const captured = await capture(testCase);
    const error = !captured.report
      ? `Edge did not emit a ready support surface report (exit=${captured.result.code})`
      : captured.report.ok
        ? null
        : (captured.report.failures ?? []).join(', ') || 'support surface probe failed';
    const summary = { ...testCase, ok: !error, error, report: captured.report };
    reports.push(summary);
    if (error) failures.push({ ...summary, dumpPath: captured.dumpPath, stderr: captured.result.stderr.slice(0, 1000) });
  }

  const output = {
    generatedAt: new Date().toISOString(),
    url: urlArg,
    smoke,
    fullMatrix,
    caseStrategy: fullMatrix ? 'cartesian-capped' : 'representative',
    caseLimit: maxRuns || cases.length,
    attemptLimit: maxAttemptsPerCase,
    matrix,
    edgeTimeoutMs,
    selectedCaseCount: selectedCases.length,
    passCount: reports.filter((item) => item.ok).length,
    failureCount: failures.length,
    failures,
    reports,
  };
  writeFileSync(join(outDir, 'summary.json'), JSON.stringify(output, null, 2));
  console.log(JSON.stringify({
    ok: failures.length === 0,
    outDir: outDir.replaceAll('\\', '/'),
    selectedCaseCount: selectedCases.length,
    passCount: output.passCount,
    failureCount: output.failureCount,
    firstFailure: failures[0] ?? null,
  }, null, 2));
  if (failures.length > 0) process.exitCode = 1;
} finally {
  await cleanupActiveChildren();
  process.off('SIGINT', onInterrupt);
  process.off('SIGTERM', onInterrupt);
}
