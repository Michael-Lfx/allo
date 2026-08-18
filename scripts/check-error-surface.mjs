/**
 * Error surface regression gate — real Vite + Arco + Edge DOM.
 *
 * Usage:
 *   bun run check:error-surface -- --url http://127.0.0.1:5173 --smoke
 *   bun run check:error-surface -- --url http://127.0.0.1:5173
 *   bun run check:error-surface -- --url http://127.0.0.1:5173 --full
 *   bun run check:error-surface -- --url http://127.0.0.1:5173 --full --max-runs 0
 *
 * The probe reports the actual rendered error card and simple Modal, default
 * disclosure state, long-detail overflow, action order/enabled state, removal
 * of the legacy side rail, and the compact default header without a visible
 * incident ID. It is deliberately separate from unit tests and from manual
 * WebView2 visual acceptance. The full matrix is capped by default; pass
 * --max-runs 0 explicitly for an uncapped run.
 */

import { existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { spawn } from 'node:child_process';

const root = new URL('..', import.meta.url).pathname.replace(/^\//, '').replaceAll('/', '\\');
const stamp = new Date().toISOString().replace(/[:.]/g, '-');
const outDir = join(root, '.superpowers', 'audits', 'artifacts', `error-surface-${stamp}`);
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
const hasMaxRuns = hasArg('--max-runs');
const requestedMaxRuns = Number(readArg('--max-runs', '0')) || 0;
const defaultFullMatrixMaxRuns = 256;
const maxRuns = hasMaxRuns ? Math.max(0, requestedMaxRuns) : fullMatrix ? defaultFullMatrixMaxRuns : 0;
const edgeTimeoutMs = Math.max(1_000, Number(readArg('--timeout-ms', '30000')) || 30_000);
const edgeKillGraceMs = 5_000;

const edgeCandidates = [
  'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
  'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
];
const edgePath = edgeCandidates.find((candidate) => existsSync(candidate));
if (!edgePath) {
  console.error('[check:error-surface] Microsoft Edge not found');
  process.exit(1);
}

const matrix = {
  widths: smoke ? [390, 1280] : [320, 360, 390, 440, 768, 1280],
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
  fixtures: smoke ? ['provider-schema', 'long-detail'] : [
    'provider-schema',
    'legacy-message',
    'long-detail',
    'no-detail',
    'config-recovery',
  ],
  expanded: smoke ? ['0', '1'] : ['0', '1'],
  scenarios: smoke ? ['off', 'normal'] : ['off', 'normal'],
};

const terminateProcessTree = (child) => {
  if (!child.pid) return;
  if (process.platform === 'win32') {
    const killer = spawn('taskkill', ['/PID', String(child.pid), '/T', '/F'], {
      stdio: 'ignore',
      windowsHide: true,
    });
    killer.on('error', () => child.kill());
    return;
  }
  child.kill('SIGKILL');
};

const run = (command, commandArgs, { timeoutMs = edgeTimeoutMs, ...spawnOptions } = {}) =>
  new Promise((resolve) => {
    const child = spawn(command, commandArgs, { windowsHide: true, ...spawnOptions });
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
      resolve({ code, signal, stdout, stderr, timedOut, spawnError });
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
      terminateProcessTree(child);
      killGraceId = setTimeout(() => finish({ signal: 'SIGKILL' }), edgeKillGraceMs);
    }, timeoutMs);
  });

const buildUrl = (query) => {
  const url = new URL(urlArg);
  url.search = new URLSearchParams(query).toString();
  url.hash = '/test/error-surface';
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
  const match = dom.match(/<pre id="error-surface-probe-result"[^>]*data-ready="true"[^>]*>([\s\S]*?)<\/pre>/i);
  if (!match) return null;
  try {
    return JSON.parse(decodeHtml(match[1].trim()));
  } catch {
    return null;
  }
};

const captureOnce = async ({ width, height, dpr, locale, scheme, theme, fixture, expanded, scenario, index, attempt }) => {
  const name = [index, width, height, dpr, locale, scheme, theme, fixture, expanded, scenario].join('-');
  const userDataDir = join(tmpdir(), `flowy-error-surface-${Date.now()}-${Math.random().toString(16).slice(2)}`);
  const dumpPath = join(outDir, `${name}${attempt ? `-retry-${attempt}` : ''}.html`);
  const screenshotPath = join(outDir, `${name}.png`);
  const url = buildUrl({ theme, locale, scheme, fixture, expanded, scenario });
  const edgeArgs = [
    '--headless',
    '--disable-gpu',
    '--disable-gpu-compositing',
    '--disable-features=VizDisplayCompositor',
    '--in-process-gpu',
    '--no-sandbox',
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
  try {
    const result = await run(edgePath, edgeArgs);
    writeFileSync(dumpPath, result.stdout);
    return { name, url, result, report: parseReport(result.stdout), dumpPath, screenshotPath };
  } finally {
    for (let cleanupAttempt = 0; cleanupAttempt < 6; cleanupAttempt += 1) {
      try {
        rmSync(userDataDir, { recursive: true, force: true, maxRetries: 2, retryDelay: 100 });
        break;
      } catch (error) {
        if (error?.code !== 'EBUSY') throw error;
        if (cleanupAttempt === 5) {
          // Edge can keep a short-lived child process attached to the isolated
          // profile after --dump-dom exits. Leave that exact temp directory for
          // the OS to release instead of turning a valid DOM report into a
          // false-negative gate.
          console.warn(`[check:error-surface] deferred cleanup for ${userDataDir}`);
          break;
        }
        await new Promise((resolve) => setTimeout(resolve, 150 * (cleanupAttempt + 1)));
      }
    }
  }
};

const capture = async (testCase) => {
  let captured;
  for (let attempt = 0; attempt < 5; attempt += 1) {
    captured = await captureOnce({ ...testCase, attempt });
    if (captured.report) return captured;
    await new Promise((resolve) => setTimeout(resolve, 250 * (attempt + 1)));
  }
  return captured;
};

const cartesianCases = () => {
  const result = [];
  let index = 0;
  for (const width of matrix.widths) {
    for (const height of matrix.heights) {
      for (const dpr of matrix.dprs) {
        for (const locale of matrix.locales) {
          for (const scheme of matrix.schemes) {
            for (const theme of matrix.themes) {
              for (const fixture of matrix.fixtures) {
                for (const expanded of matrix.expanded) {
                  for (const scenario of matrix.scenarios) {
                    result.push({ width, height, dpr, locale, scheme, theme, fixture, expanded, scenario, index: index++ });
                  }
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

const representativeCases = () => {
  const result = [];
  const seen = new Set();
  let index = 0;
  const add = (values) => {
    const key = [values.width, values.height, values.dpr, values.locale, values.scheme, values.theme, values.fixture, values.expanded, values.scenario].join('|');
    if (seen.has(key)) return;
    seen.add(key);
    result.push({ ...values, index: index++ });
  };
  const base = {
    height: matrix.heights.at(-1),
    dpr: matrix.dprs[0],
    locale: matrix.locales[0],
    scheme: matrix.schemes[0],
    theme: matrix.themes[0],
    fixture: matrix.fixtures[0],
    expanded: '0',
    scenario: matrix.scenarios[0],
  };
  for (const width of matrix.widths) add({ ...base, width });
  for (const dpr of matrix.dprs) add({ ...base, width: matrix.widths[0], dpr });
  for (const locale of matrix.locales) add({ ...base, width: matrix.widths[0], locale });
  for (const scheme of matrix.schemes) add({ ...base, width: matrix.widths[0], scheme });
  for (const theme of matrix.themes) add({ ...base, width: matrix.widths[0], theme });
  for (const fixture of matrix.fixtures) add({ ...base, width: matrix.widths[0], fixture });
  add({ ...base, width: matrix.widths[0], fixture: 'long-detail', expanded: '1' });
  add({ ...base, width: matrix.widths.at(-1), fixture: 'long-detail', expanded: '1', dpr: matrix.dprs.at(-1), locale: matrix.locales.at(-1), scheme: matrix.schemes.at(-1), theme: matrix.themes.at(-1) });
  return result;
};

const cases = fullMatrix ? cartesianCases() : representativeCases();
const selectedCases = maxRuns > 0 ? cases.slice(0, maxRuns) : cases;
const failures = [];
const reports = [];

for (const testCase of selectedCases) {
  const captured = await capture(testCase);
  const report = captured.report;
  const error = !report
    ? `Edge did not emit a ready error surface report (exit=${captured.result.code})`
    : report.ok
      ? null
      : (report.failures ?? []).join(', ') || 'error surface probe failed';
  const summary = { ...testCase, ok: !error, error, report };
  reports.push(summary);
  if (error) failures.push({ ...summary, dumpPath: captured.dumpPath, stderr: captured.result.stderr.slice(0, 1000) });
}

const output = {
  generatedAt: new Date().toISOString(),
  url: urlArg,
  smoke,
  fullMatrix,
  caseStrategy: fullMatrix ? (maxRuns > 0 ? 'cartesian-capped' : 'cartesian') : 'representative',
  caseLimit: maxRuns || null,
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

if (failures.length > 0) process.exit(1);
