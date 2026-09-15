#!/usr/bin/env node

/**
 * Keep Windows GUI hosts from flashing consoles when tools spawn processes.
 *
 * Contract:
 * 1. One-shot hide helpers live only in nomi-process-runtime
 *    (`hidden_command` / `hidden_std_command` / `apply_hidden_console*`).
 * 2. Guarded call sites must use those helpers — no local CREATE_NO_WINDOW.
 * 3. Agent shell transport must force Transport::Pipe on Windows (not ConPTY).
 */

import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');

const RUNTIME_HIDE = 'crates/shared/nomi-process-runtime/src/command_builder.rs';
const GUARDED = {
  worktree: 'crates/agent/nomi-tools/src/worktree.rs',
  lsp: 'crates/agent/nomi-tools/src/lsp.rs',
  media: 'crates/agent/nomi-media/src/video_segment.rs',
  vimaxMedia: 'crates/agent/nomi-vimax/src/media_local/mod.rs',
  grep: 'crates/agent/nomi-tools/src/grep.rs',
  shellConfig: 'crates/agent/nomi-config/src/shell.rs',
  ffmpegHw: 'crates/agent/nomi-config/src/ffmpeg_hw.rs',
  shell: 'crates/agent/nomi-tools/src/windows_shell.rs',
};

const LOCAL_HIDE_RE = /CREATE_NO_WINDOW|creation_flags\s*\(\s*0x0800_?0000\s*\)/;
const RUNTIME_HELPER_RE =
  /\b(?:hidden_command|hidden_std_command|apply_hidden_console(?:_std)?)\b/;

function readRel(rel) {
  return readFileSync(resolve(ROOT, rel), 'utf8');
}

export function checkRuntimeOwnsHide(source) {
  const problems = [];
  if (!/\bfn\s+hidden_command\s*\(/.test(source)) {
    problems.push('missing pub fn hidden_command');
  }
  if (!/\bfn\s+hidden_std_command\s*\(/.test(source)) {
    problems.push('missing pub fn hidden_std_command');
  }
  if (!/\bfn\s+apply_hidden_console\s*\(/.test(source)) {
    problems.push('missing pub fn apply_hidden_console');
  }
  if (!LOCAL_HIDE_RE.test(source) && !/CREATE_NO_WINDOW/.test(source)) {
    problems.push('runtime hide helpers must set CREATE_NO_WINDOW on Windows');
  }
  return problems;
}

export function checkCallSiteUsesRuntime(source, { requireHelper = true } = {}) {
  const problems = [];
  if (LOCAL_HIDE_RE.test(source)) {
    problems.push(
      'local CREATE_NO_WINDOW / creation_flags(0x08000000) — use nomi_process_runtime helpers',
    );
  }
  if (requireHelper && !RUNTIME_HELPER_RE.test(source)) {
    problems.push(
      'must call hidden_command / hidden_std_command / apply_hidden_console*',
    );
  }
  return problems;
}

/**
 * Agent Bash/exec_command must force Pipe on Windows. ConPTY flashes a
 * console host under GUI apps.
 */
export function checkShellTransportSource(source) {
  const problems = [];
  const fnMatch = source.match(
    /fn\s+shell_transport\s*\([^)]*\)[^{]*\{([\s\S]*?)^\}/m,
  );
  if (!fnMatch) {
    problems.push('missing shell_transport()');
    return problems;
  }
  const body = fnMatch[1];
  const forcesPipe =
    /if\s+cfg!\s*\(\s*windows\s*\)\s*\{[\s\S]*?return\s+Transport::Pipe\s*;/.test(
      body,
    );
  if (!forcesPipe) {
    problems.push(
      'shell_transport must return Transport::Pipe early when cfg!(windows)',
    );
  }
  if (/cfg!\s*\(\s*windows\s*\)\s*\|\|/.test(body)) {
    problems.push(
      'shell_transport must not combine cfg!(windows) with Pty selection',
    );
  }
  return problems;
}

export function checkAll(sources = {
  runtime: readRel(RUNTIME_HIDE),
  worktree: readRel(GUARDED.worktree),
  lsp: readRel(GUARDED.lsp),
  media: readRel(GUARDED.media),
  vimaxMedia: readRel(GUARDED.vimaxMedia),
  grep: readRel(GUARDED.grep),
  shellConfig: readRel(GUARDED.shellConfig),
  ffmpegHw: readRel(GUARDED.ffmpegHw),
  shell: readRel(GUARDED.shell),
}) {
  return [
    ...checkRuntimeOwnsHide(sources.runtime).map((p) => `${RUNTIME_HIDE}: ${p}`),
    ...checkCallSiteUsesRuntime(sources.worktree).map(
      (p) => `${GUARDED.worktree}: ${p}`,
    ),
    ...checkCallSiteUsesRuntime(sources.lsp).map((p) => `${GUARDED.lsp}: ${p}`),
    ...checkCallSiteUsesRuntime(sources.media).map(
      (p) => `${GUARDED.media}: ${p}`,
    ),
    ...checkCallSiteUsesRuntime(sources.vimaxMedia).map(
      (p) => `${GUARDED.vimaxMedia}: ${p}`,
    ),
    ...checkCallSiteUsesRuntime(sources.grep).map((p) => `${GUARDED.grep}: ${p}`),
    ...checkCallSiteUsesRuntime(sources.shellConfig).map(
      (p) => `${GUARDED.shellConfig}: ${p}`,
    ),
    ...checkCallSiteUsesRuntime(sources.ffmpegHw).map(
      (p) => `${GUARDED.ffmpegHw}: ${p}`,
    ),
    ...checkShellTransportSource(sources.shell).map(
      (p) => `${GUARDED.shell}: ${p}`,
    ),
  ];
}

function main() {
  const problems = checkAll();
  if (problems.length > 0) {
    console.error(
      'Windows console-hide contract failed (hide belongs in nomi-process-runtime):\n',
    );
    for (const problem of problems) {
      console.error(`  - ${problem}`);
    }
    console.error(
      '\nUse hidden_command / hidden_std_command / apply_hidden_console*, and keep Windows shell on Transport::Pipe.',
    );
    process.exit(1);
  }
  console.log('Windows console-hide contract OK.');
}

const isDirectRun =
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url);

if (isDirectRun) {
  main();
}
