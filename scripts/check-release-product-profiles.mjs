#!/usr/bin/env bun
/**
 * Contract gate for the shared Flowy / UniBuddy desktop release workflow.
 *
 * The two products intentionally share one platform pipeline. This checker
 * prevents a later edit from accepting the UniBuddy tag while silently using
 * Flowy branding, channel pointers, or GitHub publication behavior.
 */
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const WORKFLOW = '.github/workflows/release-modelscope.yml';
const FORBIDDEN_DUPLICATE = '.github/workflows/release-unibuddy.yml';

function stepBlocks(workflowText) {
  return workflowText.split(/\n(?=      - (?:name:|uses:))/);
}

function requireText(problems, workflowText, needle, label) {
  if (!workflowText.includes(needle)) problems.push(`missing ${label}: ${needle}`);
}

function requireStepArgs(problems, blocks, marker, requiredArgs) {
  const matching = blocks.filter((block) => block.includes(marker));
  if (matching.length === 0) {
    problems.push(`no workflow step contains ${marker}`);
    return;
  }
  for (const block of matching) {
    for (const arg of requiredArgs) {
      if (!block.includes(arg)) {
        problems.push(`${marker} step is missing ${arg}`);
      }
    }
  }
}

export function validateReleaseProductProfiles(workflowText) {
  const problems = [];
  const blocks = stepBlocks(workflowText);

  for (const [needle, label] of [
    ['- "v*.*.*"', 'Flowy tag trigger'],
    ['- "unibuddy-v*.*.*"', 'UniBuddy tag trigger'],
    ['^v([0-9]+\\.[0-9]+\\.[0-9]+)$', 'Flowy tag parser'],
    ['^unibuddy-v([0-9]+\\.[0-9]+\\.[0-9]+)$', 'UniBuddy tag parser'],
    ['profile="flowy"', 'Flowy profile'],
    ['product_name="Flowy"', 'Flowy product name'],
    ['product_config=""', 'Flowy base-config preservation'],
    ['channel_root="channels"', 'Flowy channel root'],
    ['publish_github="true"', 'Flowy GitHub publication'],
    ['profile="unibuddy"', 'UniBuddy profile'],
    ['product_name="UniBuddy"', 'UniBuddy product name'],
    ['product_config="apps/desktop/tauri.unibuddy.conf.json"', 'UniBuddy Tauri config'],
    ['channel_root="unibuddy_channels"', 'UniBuddy channel root'],
    ['windows_channel_config="apps/desktop/tauri.channel.unibuddy.windows.conf.json"', 'UniBuddy Windows channel config'],
    ['macos_channel_config="apps/desktop/tauri.channel.unibuddy.macos.conf.json"', 'UniBuddy macOS channel config'],
    ['linux_channel_config="apps/desktop/tauri.channel.unibuddy.linux.conf.json"', 'UniBuddy Linux channel config'],
    ['publish_github="false"', 'UniBuddy GitHub isolation'],
    ['permissions:\n  contents: read', 'read-only workflow default permission'],
  ]) {
    requireText(problems, workflowText, needle, label);
  }

  requireStepArgs(problems, blocks, 'make:latest --host modelscope', [
    '--product',
    '--channel-root',
  ]);
  requireStepArgs(problems, blocks, 'run-upload-modelscope.mjs', ['--channel-root']);
  requireStepArgs(problems, blocks, 'verify-modelscope-release.py', ['--channel-root']);

  for (const platform of ['win', 'mac', 'linux']) {
    requireStepArgs(problems, blocks, `bun run build:${platform}`, [
      'PRODUCT_CONFIG',
      'CHANNEL_CONFIG',
      'apps/desktop/tauri.ci-skip-ui.conf.json',
    ]);
  }

  requireStepArgs(problems, blocks, 'artifacts to GitHub Release', [
    "if: needs.release-context.outputs.publish_github == 'true'",
  ]);

  return problems;
}

export function validateCheckedInReleaseProfiles(root = ROOT) {
  const workflowPath = join(root, WORKFLOW);
  const problems = [];
  if (!existsSync(workflowPath)) return [`${WORKFLOW}: missing`];
  if (existsSync(join(root, FORBIDDEN_DUPLICATE))) {
    problems.push(`${FORBIDDEN_DUPLICATE}: duplicate product workflow must not exist`);
  }
  return problems.concat(validateReleaseProductProfiles(readFileSync(workflowPath, 'utf8')));
}

function main() {
  const problems = validateCheckedInReleaseProfiles();
  if (problems.length) {
    for (const problem of problems) console.error(`FAIL: ${problem}`);
    process.exit(1);
  }
  console.log('PASS: shared release workflow keeps Flowy and UniBuddy product profiles isolated');
}

if (import.meta.main) main();

export { FORBIDDEN_DUPLICATE, ROOT, WORKFLOW };
