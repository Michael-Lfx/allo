/**
 * Button alignment regression gate — real Vite + Arco + Icon Park + Edge DOM.
 *
 * Usage:
 *   bun run check:button-layout -- --url http://127.0.0.1:5173 --smoke
 *   bun run check:button-layout -- --url http://127.0.0.1:5173
 *   bun run check:button-layout -- --url http://127.0.0.1:5173 --full
 *
 * The default run covers every value on each viewport/DPR/locale/theme/CSS
 * axis with a bounded representative matrix. `--full` expands that into the
 * Cartesian product for exhaustive investigation; `--smoke` is a smaller
 * representative set for local iteration. Edge's --dump-dom is intentionally used instead of a browser
 * dependency: the page emits a JSON report after getComputedStyle and layout
 * measurement, so this gate can fail on the reported symptom itself.
 */

import { existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { spawn } from 'node:child_process';

const root = new URL('..', import.meta.url).pathname.replace(/^\//, '').replaceAll('/', '\\');
const stamp = new Date().toISOString().replace(/[:.]/g, '-');
const outDir = join(root, '.superpowers', 'audits', 'artifacts', `button-layout-${stamp}`);
mkdirSync(outDir, { recursive: true });

const args = process.argv.slice(2);
const readArg = (name, fallback) => {
  const index = args.indexOf(name);
  if (index === -1) return fallback;
  return args[index + 1] ?? fallback;
};
const hasArg = (name) => args.includes(name);
const urlArg = readArg('--url', 'http://127.0.0.1:5173');
const smoke = hasArg('--smoke');
const fullMatrix = hasArg('--full');
const screenshots = hasArg('--screenshots');
const maxRuns = Number(readArg('--max-runs', '0')) || 0;
const edgeTimeoutMs = Math.max(1_000, Number(readArg('--timeout-ms', '30000')) || 30_000);
const edgeKillGraceMs = 5_000;

const edgeCandidates = [
  'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
  'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
];
const edgePath = edgeCandidates.find((candidate) => existsSync(candidate));
if (!edgePath) {
  console.error('[check:button-layout] Microsoft Edge not found');
  process.exit(1);
}

const matrix = {
  widths: smoke ? [390, 560, 1280] : [320, 360, 390, 440, 560, 768, 1024, 1280],
  heights: smoke ? [900] : [720, 900],
  dprs: smoke ? [1, 1.5, 2] : [1, 1.25, 1.5, 2],
  locales: smoke ? ['zh-CN'] : ['zh-CN', 'en-US'],
  schemes: smoke ? ['light', 'dark'] : ['light', 'dark'],
  themes: smoke ? ['codex-neutral', 'rhythm-dark', 'notion'] : [
    'codex-neutral',
    'rhythm-dark',
    'neon-rainbow',
    'frosted-glass',
    'sunset-afterglow',
    'notion',
  ],
  scenarios: smoke ? ['off', 'adversarial'] : ['off', 'normal', 'adversarial'],
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
  url.hash = '/test/button-layout';
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
  const match = dom.match(/<pre id="button-layout-probe-result"[^>]*data-ready="true"[^>]*>([\s\S]*?)<\/pre>/i);
  if (!match) return null;
  try {
    return JSON.parse(decodeHtml(match[1].trim()));
  } catch {
    return null;
  }
};

const captureOnce = async ({ width, height, dpr, locale, scheme, theme, scenario, index, attempt }) => {
  const name = [index, width, height, dpr, locale, scheme, theme, scenario].join('-');
  const userDataDir = join(tmpdir(), `flowy-button-layout-${Date.now()}-${Math.random().toString(16).slice(2)}`);
  const dumpPath = join(outDir, `${name}${attempt ? `-retry-${attempt}` : ''}.html`);
  const screenshotPath = join(outDir, `${name}.png`);
  const url = buildUrl({ theme, locale, scheme, scenario });
  const edgeArgs = [
    // This machine's Edge GPU process can fail before --dump-dom exits. Keep
    // the probe deterministic in CI/dev shells that do not expose a GPU.
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
    const report = parseReport(result.stdout);
    return { name, url, result, report, dumpPath, screenshotPath };
  } finally {
    rmSync(userDataDir, { recursive: true, force: true, maxRetries: 4, retryDelay: 100 });
  }
};

const capture = async (testCase) => {
  let captured;
  for (let attempt = 0; attempt < 5; attempt += 1) {
    captured = await captureOnce({ ...testCase, attempt });
    if (captured.report) return captured;
    // A fresh Edge profile normally removes the transient empty --dump-dom
    // result seen while Vite is still compiling the first probe request.
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
              for (const scenario of matrix.scenarios) {
                result.push({ width, height, dpr, locale, scheme, theme, scenario, index: index++ });
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
    const key = [values.width, values.height, values.dpr, values.locale, values.scheme, values.theme, values.scenario].join('|');
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
    scenario: matrix.scenarios[0],
  };

  // Keep the first smoke cases useful: one normal stylesheet and one hostile
  // stylesheet, so a short run can still prove both sides of the regression.
  const firstWidth = matrix.widths[Math.min(1, matrix.widths.length - 1)];
  for (const scenario of matrix.scenarios) add({ ...base, width: firstWidth, scenario });

  for (const width of matrix.widths) add({ ...base, width });
  for (const height of matrix.heights) add({ ...base, width: firstWidth, height });
  for (const dpr of matrix.dprs) add({ ...base, width: firstWidth, dpr });
  for (const locale of matrix.locales) add({ ...base, width: firstWidth, locale, scenario: matrix.scenarios.at(-1) });
  for (const scheme of matrix.schemes) {
    const theme = scheme === 'dark' ? (matrix.themes.find((item) => item === 'rhythm-dark') ?? matrix.themes[0]) : matrix.themes[0];
    add({ ...base, width: firstWidth, dpr: matrix.dprs.at(-1), scheme, theme, scenario: matrix.scenarios.at(-1) });
  }
  for (const theme of matrix.themes) {
    const scheme = theme === 'rhythm-dark' ? 'dark' : matrix.schemes[0];
    add({ ...base, width: firstWidth, scheme, theme, scenario: matrix.scenarios.at(-1) });
  }

  // Exercise the narrow/wide hostile cases with the second locale and the
  // largest DPR, where text wrapping and fractional CSS pixels are most likely.
  if (matrix.locales.includes('en-US') && matrix.schemes.includes('dark')) {
    for (const width of [matrix.widths[0], matrix.widths.at(-1)]) {
      for (const theme of matrix.themes.slice(-2)) {
        add({
          ...base,
          width,
          dpr: matrix.dprs.at(-1),
          locale: 'en-US',
          scheme: 'dark',
          theme,
          scenario: matrix.scenarios.at(-1),
        });
      }
    }
  }
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
    ? `Edge did not emit a ready button layout report (exit=${captured.result.code})`
    : report.ok
      ? null
      : [
          ...(report.missingFixtures?.length ? [`missing-fixtures=${report.missingFixtures.join(', ')}`] : []),
          ...(report.buttons ?? [])
            .filter((button) => !button.pass)
            .map((button) => `${button.id}: ${button.failures.join(', ')}`),
        ].join('; ') || 'button layout probe failed';
  const summary = { ...testCase, ok: !error, error, styleOrder: report?.styleOrder ?? null, buttons: report?.buttons ?? null };
  reports.push(summary);
  if (error) failures.push({ ...summary, dumpPath: captured.dumpPath, stderr: captured.result.stderr.slice(0, 1000) });
}

const output = {
  generatedAt: new Date().toISOString(),
  url: urlArg,
  smoke,
  fullMatrix,
  caseStrategy: fullMatrix ? 'cartesian' : 'representative',
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
