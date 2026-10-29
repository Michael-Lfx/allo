import { expect, test } from 'bun:test';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { validateReleaseVoicePreparation } from './check-release-voice-preparation.mjs';

const BASE_HOOK =
  'bun scripts/prune-build.mjs --pre && bun scripts/ensure-bundled-rg.mjs && bun scripts/prepare-voice-companion.mjs && bun run --filter=./ui build';
const CI_HOOK =
  'bun scripts/prune-build.mjs --pre && bun scripts/ensure-bundled-rg.mjs && bun scripts/prepare-voice-companion.mjs';

function workflow({ win = true, mac = true, linux = true } = {}) {
  const block = (platform, ok) =>
    `      - name: Build ${platform}\n        run: |\n          bun run build:${platform} x64 \\\n            --config apps/desktop/tauri.updater.conf.json \\\n            ${ok ? '--config apps/desktop/tauri.ci-skip-ui.conf.json' : '--config apps/desktop/tauri.channel.conf.json'}\n`;
  return `${block('win', win)}${block('mac', mac)}${block('linux', linux)}`;
}

async function fixture({ baseHook = BASE_HOOK, ciHook = CI_HOOK, workflowText = workflow(), layered = '{}' } = {}) {
  const root = await mkdtemp(join(tmpdir(), 'flowy-release-voice-'));
  await mkdir(join(root, 'apps/desktop'), { recursive: true });
  await mkdir(join(root, '.github/workflows'), { recursive: true });
  await writeFile(join(root, 'apps/desktop/tauri.conf.json'), JSON.stringify({ build: { beforeBuildCommand: baseHook } }));
  await writeFile(join(root, 'apps/desktop/tauri.ci-skip-ui.conf.json'), JSON.stringify({ build: { beforeBuildCommand: ciHook } }));
  await writeFile(join(root, 'apps/desktop/tauri.updater.conf.json'), layered);
  await writeFile(join(root, '.github/workflows/release-modelscope.yml'), workflowText);
  return root;
}

test('the checked-in release inputs prepare the daemon and skip only the UI build', () => {
  expect(validateReleaseVoicePreparation()).toEqual([]);
});

test('a CI hook that drops voice preparation fails', async () => {
  const root = await fixture({
    ciHook: 'bun scripts/prune-build.mjs --pre && bun scripts/ensure-bundled-rg.mjs',
  });
  try {
    const problems = validateReleaseVoicePreparation(root);
    expect(problems.some((problem) => problem.includes('missing scripts/prepare-voice-companion.mjs'))).toBe(true);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('a CI hook that rebuilds the UI fails', async () => {
  const root = await fixture({ ciHook: `${CI_HOOK} && bun run --filter=./ui build` });
  try {
    expect(validateReleaseVoicePreparation(root).some((problem) => problem.includes('must not rebuild the UI'))).toBe(true);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('a release build step without the CI overlay fails', async () => {
  const root = await fixture({ workflowText: workflow({ mac: false }) });
  try {
    expect(validateReleaseVoicePreparation(root).some((problem) => problem.includes('build:mac'))).toBe(true);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('a layered release config may not override the build hook', async () => {
  const root = await fixture({ layered: JSON.stringify({ build: { beforeBuildCommand: 'bun scripts/ensure-bundled-rg.mjs' } }) });
  try {
    expect(validateReleaseVoicePreparation(root).some((problem) => problem.includes('must not be overridden'))).toBe(true);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
