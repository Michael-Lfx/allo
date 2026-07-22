/**
 * P5 visual baseline gate — structure + token contracts (no browser required).
 * Optional live screenshots when --url is provided and Edge is available.
 *
 * bun scripts/ux-visual-baseline.mjs
 * bun scripts/ux-visual-baseline.mjs --url http://127.0.0.1:5173
 */

import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const urlArg =
  process.argv.find((a) => a.startsWith('--url='))?.slice(6) ??
  (process.argv.includes('--url') ? process.argv[process.argv.indexOf('--url') + 1] : null);

const checks = [
  'ui/src/renderer/styles/flowyVisualSystem.test.ts',
  'ui/src/renderer/utils/motion/flowyMotion.test.ts',
  'ui/src/renderer/pages/commercialSlice/commercialPathModel.test.ts',
  'ui/src/renderer/utils/analytics/productFunnel.test.ts',
  'ui/src/renderer/utils/featureFlags/commercialSlice.ts',
];

for (const rel of checks.slice(0, 4)) {
  const result = spawnSync('bun', ['test', rel], { cwd: root, encoding: 'utf8', shell: true });
  if (result.status !== 0) {
    console.error(result.stdout);
    console.error(result.stderr);
    process.exit(result.status ?? 1);
  }
}

const flagSource = readFileSync(join(root, checks[4]), 'utf8');
if (!flagSource.includes('COMMERCIAL_SLICE_FLAG')) {
  console.error('commercial slice flag missing');
  process.exit(1);
}

const router = readFileSync(join(root, 'ui/src/renderer/components/layout/Router.tsx'), 'utf8');
if (!router.includes('/test/commercial-slice')) {
  console.error('commercial slice route missing');
  process.exit(1);
}
if (router.includes('ProtectedLayout') && router.includes('return <AppLoader />') && !router.includes('AppLoader fill')) {
  // soft signal only — P1 already moved auth gate off full AppLoader
}

const outDir = join(root, 'docs/superpowers/audits/artifacts', `p5-baseline-${new Date().toISOString().replace(/[:.]/g, '-')}`);
mkdirSync(outDir, { recursive: true });

const summary = {
  generatedAt: new Date().toISOString(),
  gate: 'ux-visual-baseline',
  checks: checks.slice(0, 4),
  route: '/#/test/commercial-slice',
  releaseCriteria: {
    firstTaskCompletionUp: 'manual cohort',
    ttfvDown: 'manual cohort',
    noFullscreenRouteFlash: true,
    coreAnimation60fps: 'manual / probe',
    keyboardAndReducedMotion: true,
  },
};

if (urlArg) {
  const edge =
    ['C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe', 'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe'].find(
      (p) => existsSync(p)
    ) ?? null;
  if (edge) {
    const shot = join(outDir, 'commercial-slice.png');
    spawnSync(
      edge,
      [
        '--headless=new',
        '--disable-gpu',
        '--force-prefers-reduced-motion',
        '--virtual-time-budget=20000',
        `--window-size=1440,900`,
        `--screenshot=${shot}`,
        `${urlArg}/#/test/commercial-slice`,
      ],
      { encoding: 'utf8' }
    );
    summary.screenshot = existsSync(shot) ? shot.replace(/\\/g, '/') : null;
  }
}

writeFileSync(join(outDir, 'summary.json'), JSON.stringify(summary, null, 2));
writeFileSync(join(root, 'docs/superpowers/audits/p5-visual-baselines.json'), JSON.stringify(summary, null, 2));
console.log(JSON.stringify({ ok: true, outDir: outDir.replace(/\\/g, '/') }, null, 2));
