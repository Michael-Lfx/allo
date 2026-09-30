#!/usr/bin/env node
/**
 * Motion contract for the shared Flowy scale (120 / 180 / 240ms).
 *
 * Fails when:
 *  - a `var(--flowy-motion-*)` / `var(--flowy-ease-*)` name is used but not
 *    defined in flowy-motion.css (this is how an undefined token silently
 *    drops a whole animation declaration)
 *  - the reduced-motion fallback no longer collapses infinite animations
 *  - the app sider animates width
 *  - primary nav rows use transition-all
 *  - marketplace card shells paint a card-level hover border
 *
 * Repo-wide `transition-all` and off-scale durations are still widespread.
 * This script ratchets `transition-all` so the count cannot grow.
 *
 * Usage: bun scripts/check-motion.mjs
 */
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, dirname, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const RENDERER = join(ROOT, 'ui/src/renderer');
const MOTION_CSS = join(RENDERER, 'styles/flowy-motion.css');
const LAYOUT_CSS = join(RENDERER, 'styles/layout.css');
const SIDER_NAV = join(RENDERER, 'components/layout/Sider/SiderNav');
const MARKET_SHELL = join(RENDERER, 'pages/settings/skill/MarketCardShell.tsx');

/** Raise only by deleting call sites, never by adding them. */
const TRANSITION_ALL_CEILING = 82;

const failures = [];

const fail = (message) => {
  failures.push(message);
};

const walk = (dir, acc = []) => {
  for (const entry of readdirSync(dir)) {
    if (entry === 'node_modules' || entry === 'dist') continue;
    const path = join(dir, entry);
    const stat = statSync(path);
    if (stat.isDirectory()) walk(path, acc);
    else if (/\.(css|tsx?)$/.test(entry) && !/\.test\.tsx?$/.test(entry)) acc.push(path);
  }
  return acc;
};

const motionCss = readFileSync(MOTION_CSS, 'utf8');
const defined = new Set();
for (const match of motionCss.matchAll(/(--flowy-(?:motion|ease)-[a-z0-9-]+)\s*:/g)) {
  defined.add(match[1]);
}

if (defined.size < 6) {
  fail(`flowy-motion.css should define the duration and easing tokens, found ${defined.size}`);
}

const used = new Map();
for (const file of walk(RENDERER)) {
  const text = readFileSync(file, 'utf8');
  for (const match of text.matchAll(/var\(\s*(--flowy-(?:motion|ease)-[a-z0-9-]+)/g)) {
    const name = match[1];
    if (!defined.has(name)) {
      const rel = relative(ROOT, file).replaceAll('\\', '/');
      const list = used.get(name) ?? [];
      list.push(rel);
      used.set(name, list);
    }
  }
}

for (const [name, files] of used) {
  const sample = [...new Set(files)].slice(0, 8).join(', ');
  fail(`${name} is used but not defined in styles/flowy-motion.css (${sample})`);
}

if (!motionCss.includes('prefers-reduced-motion: reduce')) {
  fail('flowy-motion.css is missing a prefers-reduced-motion fallback');
}
if (!motionCss.includes('animation-iteration-count: 1 !important')) {
  fail('reduced motion must stop infinite animations (animation-iteration-count: 1)');
}
if (!motionCss.includes('animation-duration: 0.01ms !important')) {
  fail('reduced motion must collapse animation duration');
}

const layoutCss = readFileSync(LAYOUT_CSS, 'utf8');
if (/transition:\s*width/.test(layoutCss)) {
  fail('styles/layout.css still transitions width; the sider must snap');
}
if (!layoutCss.includes('.arco-layout-sider.layout-sider') || !/\.arco-layout-sider\.layout-sider\s*\{[^}]*transition:\s*none/.test(layoutCss)) {
  fail('styles/layout.css must disable Arco sider width transitions');
}
if (/grid-template-rows\s+\d/.test(layoutCss) && /transition:[^;]*grid-template-rows/.test(layoutCss)) {
  fail('styles/layout.css still transitions grid-template-rows');
}

const market = readFileSync(MARKET_SHELL, 'utf8');
if (/hover:border/.test(market)) {
  fail('MarketCardShell must not paint a card-level hover border');
}

for (const file of walk(SIDER_NAV)) {
  const text = readFileSync(file, 'utf8');
  if (text.includes('transition-all')) {
    fail(`${relative(ROOT, file).replaceAll('\\', '/')} uses transition-all`);
  }
}

let transitionAll = 0;
for (const file of walk(RENDERER)) {
  const text = readFileSync(file, 'utf8');
  transitionAll += text.split('transition-all').length - 1;
}
if (transitionAll > TRANSITION_ALL_CEILING) {
  fail(
    `transition-all count is ${transitionAll}, above the ceiling ${TRANSITION_ALL_CEILING}. ` +
      'Replace it with explicit properties (color, background-color, opacity, transform).'
  );
}

if (failures.length > 0) {
  console.error(`check:motion failed (${failures.length})`);
  for (const message of failures) console.error(`  - ${message}`);
  console.error(`transition-all count: ${transitionAll} (ceiling ${TRANSITION_ALL_CEILING})`);
  process.exit(1);
}

console.log(`check:motion ok (transition-all ${transitionAll}/${TRANSITION_ALL_CEILING})`);
