/**
 * migrate-arco-message-imports.mjs — one-off codemod that moves every runtime
 * Arco `Message` / `Notification` named import onto the unified notifications
 * facade (`@/renderer/components/notifications`) under an alias, so call sites
 * keep working unchanged:
 *
 *   import { Button, Message } from '@arco-design/web-react';
 *   →
 *   import { Button } from '@arco-design/web-react';
 *   import { AppMessage as Message } from '@/renderer/components/notifications';
 *
 * Usage:
 *   bun ui/scripts/migrate-arco-message-imports.mjs           # dry-run (default): report only
 *   bun ui/scripts/migrate-arco-message-imports.mjs --write   # apply changes
 *
 * Idempotent: a second run finds no matching members and rewrites nothing.
 * Files needing human judgement (aliases, `import type`, type-surface
 * rewrites) are skipped and listed in the report instead of being touched.
 */

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const uiRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const ROOT = path.join(uiRoot, 'src', 'renderer');
const WRITE = process.argv.includes('--write');

const ARCO_IMPORT_RE = /import[ \t]+(type[ \t]+)?\{([^}]*)\}[ \t]*from[ \t]*['"]@arco-design\/web-react['"];?[ \t]*\r?\n?/g;
const TARGETS = new Set(['Message', 'Notification']);
const ALIAS_RE = /^(Message|Notification)\s+as\s+(\w+)$/;
const FACADE_SOURCE = "@/renderer/components/notifications";

// Repo-relative (posix, relative to src/renderer) files migrated by hand.
const MANUAL_FILES = new Set([
  'pages/knowledge/KnowledgeConsumersSection.tsx', // alias `Message as ArcoMessage` + icon-park `Message`
  'hooks/preset/usePresetEditor.ts', // import type + ReturnType<...useMessage> surface
  'pages/conversation/Workspace/types.ts', // import type + MessageApi surface
  'pages/settings/PresetSettings/SkillConfirmModals.tsx', // import type + prop surface
  'components/settings/SettingsModal/contents/ToolsModalContent.tsx', // value import, type-only usage
  'pages/conversation/Preview/hooks/usePreviewHistory.ts', // value import, type-only usage
]);

const walk = function* (dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === 'node_modules' || entry.name === 'dist') continue;
      yield* walk(full);
      continue;
    }
    if (/\.tsx?$/.test(entry.name)) yield full;
  }
};

const lineOf = (src, index) => src.slice(0, index).split('\n').length;

const parseMembers = (braces) =>
  braces
    .split(',')
    .map((member) => member.trim())
    .filter(Boolean);

const findSuspiciousPatterns = (src, rel) => {
  const suspicions = [];
  const push = (index, label) => suspicions.push({ rel, line: lineOf(src, index), label });

  for (const match of src.matchAll(/\bMessage\.(config|useMessage|clear)\b/g)) {
    push(match.index, `Message.${match[1]} — verify facade semantics`);
  }
  for (const match of src.matchAll(/\btypeof\s+(Message|Notification)\b/g)) {
    push(match.index, `typeof ${match[1]} — verify type compatibility`);
  }
  for (const match of src.matchAll(/\b(Message|Notification)\.(info|success|warning|error|loading|normal)\(\s*['"`][^\n]*?['"`]\s*,/g)) {
    push(match.index, `${match[1]}.${match[2]}('...', ...) — antd-style two-arg call, facade takes a single config`);
  }
  for (const match of src.matchAll(/\b(Message|Notification)\.(info|success|warning|error|loading|normal)\(\s*\{/g)) {
    const window = src.slice(match.index, match.index + 800);
    const keyMatch = window.match(/^\s*(className|style|position|getPopupContainer)\s*:/m);
    if (keyMatch) {
      push(match.index, `${match[1]}.${match[2]}({ ... ${keyMatch[1]}: ... }) — unsupported config key nearby (heuristic)`);
    }
  }
  return suspicions;
};

const report = {
  scanned: 0,
  rewritten: [],
  skipped: [],
  tests: [],
  suspicions: [],
};

for (const file of walk(ROOT)) {
  const rel = path.relative(ROOT, file).split(path.sep).join('/');
  report.scanned += 1;
  const src = fs.readFileSync(file, 'utf8');

  const matches = [...src.matchAll(ARCO_IMPORT_RE)];
  const targets = matches.filter((match) => parseMembers(match[2]).some((member) => TARGETS.has(member) || ALIAS_RE.test(member)));
  if (targets.length === 0) continue;

  if (path.basename(file).includes('.test.')) {
    report.tests.push(rel);
    continue;
  }

  // ---- skip guards (whole file) ----
  let skipReason = null;
  if (MANUAL_FILES.has(rel)) skipReason = 'manual migration (alias/type surface)';
  if (!skipReason && targets.some((match) => match[1])) skipReason = 'import type — migrate to AppMessageInstance manually';
  if (!skipReason && targets.some((match) => match[2].includes('//') || match[2].includes('/*'))) {
    skipReason = 'comment inside import braces';
  }
  if (!skipReason) {
    for (const match of targets) {
      const alias = parseMembers(match[2]).find((member) => ALIAS_RE.test(member));
      if (alias) {
        skipReason = `aliased member '${alias}'`;
        break;
      }
    }
  }
  if (!skipReason && targets.some((match) => parseMembers(match[2]).some((member) => member.startsWith('type ')))) {
    skipReason = 'inline `type` modifier in braces';
  }
  if (!skipReason && src.includes(`from '${FACADE_SOURCE}'`)) skipReason = 'facade import already present — dedupe manually';
  if (skipReason) {
    report.skipped.push({ rel, reason: skipReason });
    continue;
  }

  // ---- rewrite, right to left so earlier offsets stay valid ----
  let next = src;
  const removedNames = new Set();
  let insertPos = null;
  let droppedAny = false;
  let multilineAny = false;
  for (const match of targets.slice().sort((a, b) => b.index - a.index)) {
    const members = parseMembers(match[2]);
    const removed = members.filter((member) => TARGETS.has(member));
    const kept = members.filter((member) => !TARGETS.has(member));
    removed.forEach((name) => removedNames.add(name));

    let replacement;
    if (kept.length === 0) {
      replacement = '';
      droppedAny = true;
    } else if (match[2].includes('\n')) {
      multilineAny = true;
      const lines = match[0].split('\n').filter((line) => {
        const trimmed = line.trim();
        return trimmed !== 'Message,' && trimmed !== 'Message' && trimmed !== 'Notification,' && trimmed !== 'Notification';
      });
      replacement = lines.join('\n');
    } else {
      const trailing = match[0].endsWith('\r\n') ? '\r\n' : match[0].endsWith('\n') ? '\n' : '';
      replacement = `import { ${kept.join(', ')} } from '@arco-design/web-react';${trailing}`;
    }
    next = next.slice(0, match.index) + replacement + next.slice(match.index + match[0].length);
    insertPos = match.index + replacement.length;
  }

  const aliases = [];
  if (removedNames.has('Message')) aliases.push('AppMessage as Message');
  if (removedNames.has('Notification')) aliases.push('AppNotification as Notification');
  const facadeLine = `import { ${aliases.join(', ')} } from '${FACADE_SOURCE}';\n`;
  next = next.slice(0, insertPos) + facadeLine + next.slice(insertPos);

  report.suspicions.push(...findSuspiciousPatterns(next, rel));
  report.rewritten.push({ rel, removed: [...removedNames].join('+'), droppedArco: droppedAny, multiline: multilineAny });
  if (WRITE) fs.writeFileSync(file, next, 'utf8');
}

// ---- report ----
console.log(`migrate-arco-message-imports — scanned ${report.scanned} files under src/renderer`);
console.log(`MODE: ${WRITE ? 'write' : 'dry-run (pass --write to apply)'}\n`);

console.log(`REWRITTEN (${report.rewritten.length}):`);
for (const item of report.rewritten) {
  const note = item.droppedArco ? 'arco import dropped (no members left)' : `arco import kept${item.multiline ? ' (multiline)' : ''}`;
  console.log(`  ${item.rel}  removed: ${item.removed}  ${note}`);
}

console.log(`\nSKIPPED (${report.skipped.length}):`);
for (const item of report.skipped) console.log(`  ${item.rel}  reason: ${item.reason}`);

console.log(`\nTEST FILES CONTAINING THE ARCO IMPORT PATTERN — NOT rewritten, update assertions manually (${report.tests.length}):`);
for (const rel of report.tests) console.log(`  ${rel}`);

console.log(`\nSUSPICIONS FOR MANUAL REVIEW (${report.suspicions.length}):`);
for (const item of report.suspicions) console.log(`  ${item.rel}:${item.line}  ${item.label}`);
