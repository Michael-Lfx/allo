#!/usr/bin/env node

/**
 * Prevent Windows GUI hosts from flashing console windows when agent tools
 * spawn git / LSP / ffmpeg, and keep agent shell on Pipe (not ConPTY).
 *
 * Guarded regressions:
 * - worktree/lsp/video_segment must hide consoles via CREATE_NO_WINDOW
 * - windows_shell::shell_transport must force Transport::Pipe on Windows
 */

import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');

const GUARDED = {
  worktree: 'crates/agent/nomi-tools/src/worktree.rs',
  lsp: 'crates/agent/nomi-tools/src/lsp.rs',
  media: 'crates/agent/nomi-media/src/video_segment.rs',
  shell: 'crates/agent/nomi-tools/src/windows_shell.rs',
};

const CREATE_NO_WINDOW_RE = /CREATE_NO_WINDOW|creation_flags\s*\(/;
const COMMAND_NEW_RE = /(?:tokio::process::|std::process::)?Command::new\s*\(/g;

function readRel(rel) {
  return readFileSync(resolve(ROOT, rel), 'utf8');
}

function lineOf(source, index) {
  return source.slice(0, index).split(/\r?\n/).length;
}

/**
 * Every Command::new outside a hide helper must sit near creation_flags.
 * Helpers named *command / no_window_* that themselves set CREATE_NO_WINDOW
 * may construct Command::new once.
 */
export function findBareCommandSpawns(source, { helperNameRe } = {}) {
  const problems = [];
  const helperRanges = [];

  if (helperNameRe) {
    const fnRe = new RegExp(
      String.raw`fn\s+(${helperNameRe.source})\s*\([^)]*\)[^{]*\{`,
      'g',
    );
    let match;
    while ((match = fnRe.exec(source)) !== null) {
      const start = match.index;
      const bodyStart = source.indexOf('{', start);
      if (bodyStart < 0) continue;
      let depth = 0;
      let end = bodyStart;
      for (; end < source.length; end += 1) {
        const ch = source[end];
        if (ch === '{') depth += 1;
        else if (ch === '}') {
          depth -= 1;
          if (depth === 0) {
            end += 1;
            break;
          }
        }
      }
      const body = source.slice(bodyStart, end);
      if (CREATE_NO_WINDOW_RE.test(body)) {
        helperRanges.push([start, end]);
      }
    }
  }

  let match;
  COMMAND_NEW_RE.lastIndex = 0;
  while ((match = COMMAND_NEW_RE.exec(source)) !== null) {
    const at = match.index;
    if (helperRanges.some(([start, end]) => at >= start && at < end)) {
      continue;
    }
    const window = source.slice(at, Math.min(source.length, at + 600));
    if (CREATE_NO_WINDOW_RE.test(window)) {
      continue;
    }
    problems.push({
      line: lineOf(source, at),
      snippet: source.slice(at, at + 48).replace(/\s+/g, ' '),
    });
  }
  return problems;
}

export function checkWorktreeSource(source) {
  const problems = [];
  if (!/fn\s+git_command\s*\(/.test(source)) {
    problems.push('missing git_command() hide helper');
  }
  if (!CREATE_NO_WINDOW_RE.test(source)) {
    problems.push('missing CREATE_NO_WINDOW / creation_flags');
  }
  for (const hit of findBareCommandSpawns(source, {
    helperNameRe: /git_command/,
  })) {
    problems.push(`bare Command::new at line ${hit.line}: ${hit.snippet}`);
  }
  return problems;
}

export function checkLspSource(source) {
  const problems = [];
  if (!/tokio::process::Command::new\s*\(/.test(source)) {
    problems.push('expected tokio::process::Command::new for LSP spawn');
  }
  if (!CREATE_NO_WINDOW_RE.test(source)) {
    problems.push('missing CREATE_NO_WINDOW / creation_flags near LSP spawn');
  }
  for (const hit of findBareCommandSpawns(source)) {
    problems.push(`bare Command::new at line ${hit.line}: ${hit.snippet}`);
  }
  return problems;
}

export function checkMediaSource(source) {
  const problems = [];
  if (!/fn\s+media_command\s*\(/.test(source)) {
    problems.push('missing media_command() hide helper');
  }
  if (!CREATE_NO_WINDOW_RE.test(source)) {
    problems.push('missing CREATE_NO_WINDOW / creation_flags');
  }
  for (const hit of findBareCommandSpawns(source, {
    helperNameRe: /media_command/,
  })) {
    problems.push(`bare Command::new at line ${hit.line}: ${hit.snippet}`);
  }
  return problems;
}

/**
 * Agent Bash/exec_command must force Pipe on Windows. ConPTY flashes a
 * console host under GUI apps (regression from forcing Transport::Pty).
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
  worktree: readRel(GUARDED.worktree),
  lsp: readRel(GUARDED.lsp),
  media: readRel(GUARDED.media),
  shell: readRel(GUARDED.shell),
}) {
  return [
    ...checkWorktreeSource(sources.worktree).map(
      (p) => `${GUARDED.worktree}: ${p}`,
    ),
    ...checkLspSource(sources.lsp).map((p) => `${GUARDED.lsp}: ${p}`),
    ...checkMediaSource(sources.media).map((p) => `${GUARDED.media}: ${p}`),
    ...checkShellTransportSource(sources.shell).map(
      (p) => `${GUARDED.shell}: ${p}`,
    ),
  ];
}

function main() {
  const problems = checkAll();
  if (problems.length > 0) {
    console.error(
      'Windows console-hide contract failed (agent tool spawns must stay hidden):\n',
    );
    for (const problem of problems) {
      console.error(`  - ${problem}`);
    }
    console.error(
      '\nUse a hide helper (CREATE_NO_WINDOW / creation_flags) or keep Windows shell on Transport::Pipe.',
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
