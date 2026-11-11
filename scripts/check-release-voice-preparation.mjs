#!/usr/bin/env bun
/**
 * Release voice preparation contract.
 *
 * The release workflow hands Tauri `tauri.ci-skip-ui.conf.json` so the
 * pre-built `ui/dist` is not rebuilt. That override used to replace the whole
 * `beforeBuildCommand`, which silently dropped `prepare-voice-companion.mjs`
 * and shipped installers without the voice daemon (v1.6.9).
 *
 * This gate reads the real configs and the real release workflow and fails if:
 *   - either build hook stops preparing the voice daemon or bundled rg;
 *   - the CI hook starts rebuilding the UI;
 *   - a layered release config overrides `beforeBuildCommand`;
 *   - any release build command stops passing the CI overlay.
 *
 *   bun run check:release-voice-preparation
 */
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const BASE_CONF = 'apps/desktop/tauri.conf.json';
const CI_CONF = 'apps/desktop/tauri.ci-skip-ui.conf.json';
const RELEASE_WORKFLOW = '.github/workflows/release-modelscope.yml';
const PREPARE = 'scripts/prepare-voice-companion.mjs';
const RG = 'scripts/ensure-bundled-rg.mjs';
const PRUNE = 'scripts/prune-build.mjs';
const REQUIRED = [PREPARE, RG, PRUNE];

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'));
}

export function hookParts(command) {
  return String(command ?? '')
    .split('&&')
    .map((part) => part.trim())
    .filter(Boolean);
}

function isUiBuild(part) {
  return /(?:^|\s)(?:bun\s+)?run\s+--filter=\.\/ui\s+build(?:\s|$)/.test(part) || /\bui build\b/.test(part);
}

const NON_RELEASE_CONFS = new Set(['tauri.conf.json', 'tauri.dev.conf.json', 'tauri.ci-skip-ui.conf.json']);

/** Every layered config that the release wrappers apply must leave the hook alone. */
export function overriddenHooks(desktopDir) {
  const problems = [];
  for (const name of readdirSync(desktopDir)) {
    if (!name.endsWith('.conf.json') || NON_RELEASE_CONFS.has(name)) continue;
    const path = join(desktopDir, name);
    const config = readJson(path);
    if (config?.build?.beforeBuildCommand !== undefined) {
      problems.push(`${name}: build.beforeBuildCommand must not be overridden by a layered config`);
    }
  }
  return problems;
}

export function workflowCallsCiConfig(workflowText, platform) {
  const marker = `bun run build:${platform}`;
  let index = workflowText.indexOf(marker);
  let seen = 0;
  while (index !== -1) {
    seen += 1;
    const nextStep = workflowText.indexOf('\n      - ', index + marker.length);
    const window = workflowText.slice(index, nextStep === -1 ? undefined : nextStep);
    if (!window.includes(`--config ${CI_CONF}`)) return false;
    index = workflowText.indexOf(marker, index + marker.length);
  }
  return seen > 0;
}

export function validateReleaseVoicePreparation(root = ROOT) {
  const problems = [];
  const base = readJson(join(root, BASE_CONF));
  const ci = readJson(join(root, CI_CONF));
  const baseHook = base?.build?.beforeBuildCommand;
  const effectiveHook = ci?.build?.beforeBuildCommand ?? baseHook;
  const baseParts = hookParts(baseHook);
  const effectiveParts = hookParts(effectiveHook);

  for (const required of REQUIRED) {
    if (!baseParts.some((part) => part.includes(required))) {
      problems.push(`${BASE_CONF}: beforeBuildCommand is missing ${required}`);
    }
    if (!effectiveParts.some((part) => part.includes(required))) {
      problems.push(`${CI_CONF}: effective beforeBuildCommand is missing ${required}`);
    }
  }

  if (!baseParts.some(isUiBuild)) {
    problems.push(`${BASE_CONF}: local beforeBuildCommand should still build the UI`);
  }
  if (effectiveParts.some(isUiBuild)) {
    problems.push(`${CI_CONF}: CI builds must not rebuild the UI (ui/dist is downloaded)`);
  }

  const missingFromCi = baseParts.filter((part) => !isUiBuild(part) && !effectiveParts.includes(part));
  if (missingFromCi.length) {
    problems.push(`${CI_CONF}: CI hook drops local preparation step(s): ${missingFromCi.join(' | ')}`);
  }

  problems.push(...overriddenHooks(join(root, 'apps/desktop')));

  const workflowPath = join(root, RELEASE_WORKFLOW);
  if (!existsSync(workflowPath)) {
    problems.push(`${RELEASE_WORKFLOW}: missing`);
  } else {
    const workflow = readFileSync(workflowPath, 'utf8');
    for (const platform of ['win', 'mac', 'linux']) {
      if (!workflowCallsCiConfig(workflow, platform)) {
        problems.push(`${RELEASE_WORKFLOW}: build:${platform} must pass --config ${CI_CONF}`);
      }
    }
  }

  return problems;
}

function main() {
  const problems = validateReleaseVoicePreparation();
  if (problems.length) {
    for (const problem of problems) console.error(`FAIL: ${problem}`);
    process.exit(1);
  }
  console.log('PASS: release builds prepare the production voice daemon without rebuilding the UI');
}

if (import.meta.main) main();

export { ROOT, BASE_CONF, CI_CONF, RELEASE_WORKFLOW };
