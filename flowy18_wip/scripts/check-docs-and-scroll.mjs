#!/usr/bin/env bun
/**
 * Flowy 1.8 static doc, ScrollArea anti-regression, and a11y key contracts.
 * Companion to check-ui-contracts.mjs; safe to run with Bun or Node.
 */
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import assert from "node:assert/strict";

const defaultRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

// Shrink-only allowlist of legacy raw scroll viewports outside flowy-ui/src/scroll.rs.
// New files and migrated ScrollArea viewports must stay at 0.
const rawScrollAllowlist = new Map([
  ["flowy-ui/src/overlays.rs", 1],
  ["flowy-app/src/app/appearance.rs", 1],
  ["flowy-app/src/app/gallery.rs", 2],
  ["flowy-app/src/app/invite.rs", 1],
  ["flowy-app/src/app/login.rs", 2],
  ["flowy-app/src/app/more/personalization.rs", 1],
  ["flowy-app/src/app/more.rs", 1],
  ["flowy-app/src/app/projects.rs", 1],
  ["flowy-app/src/app/settings.rs", 3],
  ["flowy-app/src/app/skills/mine.rs", 2],
  ["flowy-app/src/app/skills.rs", 3],
  ["flowy-studio/src/automation.rs", 1],
  ["flowy-studio/src/automation_page.rs", 1],
  ["flowy-studio/src/composer_add_menu.rs", 1],
  ["flowy-studio/src/drawer_preview.rs", 1],
  ["flowy-studio/src/home.rs", 1],
  ["flowy-studio/src/library/preview_view.rs", 1],
  ["flowy-studio/src/markdown.rs", 2],
  ["flowy-studio/src/pending_card.rs", 2],
  ["flowy-studio/src/router_works.rs", 1],
  ["flowy-studio/src/studio.rs", 1],
]);

// Shrink-only baseline of historical docs/plans links predating this check.
// All live docs (AGENTS.md, DESIGN.md, docs/architecture/**, new plans) require 0.
const legacyDocLinkAllowlist = new Map([
  ["docs/plans/local-agent-design.zh.md", 18],
  ["docs/plans/conversation-artifact-library.zh.md", 3],
  ["docs/plans/local-first-ui-v2.zh.md", 5],
]);

function codeOnly(source) {
  const out = source.split("");
  const blank = (a, b) => {
    for (let j = a; j < b; j++) if (out[j] !== "\n") out[j] = " ";
  };
  for (let i = 0; i < source.length;) {
    if (source.startsWith("//", i)) {
      const end = source.indexOf("\n", i);
      const next = end < 0 ? source.length : end;
      blank(i, next);
      i = next;
    } else if (source.startsWith("/*", i)) {
      const start = i;
      let depth = 1;
      i += 2;
      while (i < source.length && depth) {
        if (source.startsWith("/*", i)) {
          depth++;
          i += 2;
        } else if (source.startsWith("*/", i)) {
          depth--;
          i += 2;
        } else i++;
      }
      blank(start, i);
    } else {
      const raw = source.slice(i).match(/^(?:br|r)(#*)"/);
      if (raw) {
        const endMark = `"${raw[1]}`;
        const end = source.indexOf(endMark, i + raw[0].length);
        const next = end < 0 ? source.length : end + endMark.length;
        blank(i, next);
        i = next;
      } else if (source[i] === '"') {
        const start = i++;
        while (i < source.length) {
          if (source[i] === "\\") i += 2;
          else if (source[i++] === '"') break;
        }
        blank(start, i);
      } else if (source[i] === "'" && /^'(?:[^'\\]|\\(?:.|u\{[^}]+\}))'/.test(source.slice(i))) {
        const literal = source.slice(i).match(/^'(?:[^'\\]|\\(?:.|u\{[^}]+\}))'/)[0];
        blank(i, i + literal.length);
        i += literal.length;
      } else i++;
    }
  }
  return out.join("");
}

function closing(code, start) {
  const pair = { "(": ")", "[": "]", "{": "}" };
  const stack = [];
  for (let i = start; i < code.length; i++) {
    if (pair[code[i]]) stack.push(pair[code[i]]);
    else if (code[i] === stack.at(-1)) {
      stack.pop();
      if (!stack.length) return i;
    }
  }
  return code.length - 1;
}

function testItemEnd(masked, attrStart) {
  let cursor = attrStart;
  while (cursor < masked.length) {
    while (cursor < masked.length && /\s/.test(masked[cursor])) cursor++;
    if (masked[cursor] === "#") {
      const rest = masked.slice(cursor + 1).match(/^\s*\[/);
      if (rest) {
        cursor = closing(masked, cursor + 1 + rest[0].length - 1) + 1;
        continue;
      }
    }
    break;
  }
  const rest = masked.slice(cursor);
  const vis = "(?:pub(?:\\s*\\([^)]*\\))?\\s+)?";
  const qual = "(?:(?:default|unsafe|async|const)\\s+)*";
  const isBraceItem =
    masked[cursor] === "{" ||
    new RegExp(`^${vis}${qual}(?:mod|fn|impl|trait|struct|enum|union)\\b`).test(rest) ||
    /^macro_rules\s*!/.test(rest);
  let depth = 0;
  for (let i = cursor; i < masked.length; i++) {
    const ch = masked[i];
    if (isBraceItem) {
      if (ch === "(" || ch === "[") depth++;
      else if ((ch === ")" || ch === "]") && depth > 0) depth--;
      else if (depth === 0) {
        if (ch === ";") return i + 1;
        if (ch === "{") return closing(masked, i) + 1;
      }
    } else {
      if (ch === "(" || ch === "[" || ch === "{") depth++;
      else if (ch === ")" || ch === "]") {
        if (depth > 0) depth--;
      } else if (ch === "}") {
        if (depth > 0) depth--;
        else return i;
      } else if (depth === 0 && (ch === ";" || ch === ",")) {
        return i + 1;
      }
    }
  }
  return masked.length;
}

export function stripTestCode(source) {
  const masked = codeOnly(source);
  const out = source.split("");
  let coveredUntil = 0;
  for (const match of masked.matchAll(/#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]/g)) {
    if (match.index < coveredUntil) continue;
    const end = testItemEnd(masked, match.index + match[0].length);
    for (let i = match.index; i < end; i++) {
      if (out[i] !== "\n") out[i] = " ";
    }
    coveredUntil = end;
  }
  return out.join("");
}

export function inspectScrollUsage(path, source) {
  if (path === "flowy-ui/src/scroll.rs") return [];
  const production = stripTestCode(source);
  const code = codeOnly(production);
  const issues = [];
  const add = (offset, message) =>
    issues.push(`${path}:${production.slice(0, offset).split("\n").length}: ${message}`);

  const rawMatches = [...code.matchAll(/\.(overflow_y_scroll|overflow_x_scroll|overflow_scroll)\s*\(/g)];
  const allowed = rawScrollAllowlist.get(path) ?? 0;
  if (rawMatches.length > allowed) {
    const first = rawMatches[allowed] ?? rawMatches[0];
    add(
      first.index,
      `raw .${first[1]}() exceeds ScrollArea baseline (${rawMatches.length} > ${allowed}); use flowy_ui::{scroll_area, scroll_list_host}`,
    );
  }
  for (const match of code.matchAll(/\.scrollbar_width\s*\(/g)) {
    add(match.index, "do not set .scrollbar_width() outside flowy-ui/src/scroll.rs");
  }
  return issues;
}

export function inspectMarkdownContent(relPath, rawBytesOrText, baseDir, pathExists = existsSync) {
  const rawBuffer = typeof rawBytesOrText === "string" ? Buffer.from(rawBytesOrText, "utf8") : rawBytesOrText;
  const issues = [];
  if (rawBuffer[0] === 0xef && rawBuffer[1] === 0xbb && rawBuffer[2] === 0xbf) {
    issues.push(`${relPath}:1: UTF-8 BOM is forbidden in markdown docs`);
  }
  if (rawBuffer.includes(0x0d)) {
    issues.push(`${relPath}: CRLF (\\r) line endings are forbidden in markdown docs`);
  }

  const text = rawBuffer.toString("utf8").replace(/^\uFEFF/, "");
  const lines = text.split(/\r?\n/);
  let openFence = null;
  const linkIssues = [];

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const fenceMatch = line.match(/^\s*(`{3,}|~{3,})(.*)$/);
    if (fenceMatch) {
      const marker = fenceMatch[1];
      if (!openFence) {
        openFence = { char: marker[0], len: marker.length, line: i + 1 };
      } else if (marker[0] === openFence.char && marker.length >= openFence.len && !fenceMatch[2].trim()) {
        openFence = null;
      }
      continue;
    }
    if (openFence) continue;

    const stripped = line.replace(/`[^`]*`/g, "");
    for (const match of stripped.matchAll(/!?\[[^\]]*\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g)) {
      const target = match[1];
      if (/^(?:https?:|mailto:|#|<https?:)/.test(target)) continue;
      if (/^(?:file:|\/|[a-zA-Z]:[\\/])/.test(target)) {
        linkIssues.push(`${relPath}:${i + 1}: machine-local or root-absolute link is forbidden: ${target}`);
        continue;
      }
      const clean = target.split("#", 1)[0].split("?", 1)[0];
      if (!clean) continue;
      let decoded = clean;
      try { decoded = decodeURIComponent(clean); } catch { /* keep literal */ }
      if (!pathExists(resolve(baseDir, decoded))) {
        linkIssues.push(`${relPath}:${i + 1}: broken relative markdown link: ${target}`);
      }
    }
  }

  if (openFence) {
    issues.push(`${relPath}:${openFence.line}: unclosed fenced code block`);
  }
  const allowedLinks = legacyDocLinkAllowlist.get(relPath) ?? 0;
  if (linkIssues.length > allowedLinks) {
    issues.push(...linkIssues.slice(allowedLinks));
  }
  return issues;
}

export function inspectLocaleNaming(relPath, messages) {
  const issues = [];
  if (messages === null || Array.isArray(messages) || typeof messages !== "object") {
    return [`${relPath}: locale dictionary must be an object`];
  }
  for (const key of Object.keys(messages)) {
    if (key.startsWith("a11y.") && !/^a11y\.[a-z0-9_]+$/.test(key)) {
      issues.push(`${relPath}: a11y key '${key}' must use lowercase snake_case after 'a11y.'`);
    }
  }
  return issues;
}

function collectFiles(directory, extension) {
  if (!existsSync(directory)) return [];
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    if (entry.name === "node_modules" || entry.name === "target" || entry.name === "build.noindex") return [];
    const full = join(directory, entry.name);
    return entry.isDirectory() ? collectFiles(full, extension) : entry.name.endsWith(extension) ? [full] : [];
  });
}

export function runChecks(root = defaultRoot) {
  const issues = [];
  const cratesRoot = join(root, "crates");
  for (const crate of ["flowy-ui", "flowy-app", "flowy-studio"]) {
    for (const file of collectFiles(join(cratesRoot, crate, "src"), ".rs")) {
      const rel = relative(cratesRoot, file).replaceAll("\\", "/");
      issues.push(...inspectScrollUsage(rel, readFileSync(file, "utf8")));
    }
  }

  const mdFiles = [
    ...["AGENTS.md", "README.md", "DESIGN.md"]
      .map((name) => join(root, name))
      .filter((p) => existsSync(p)),
    ...collectFiles(join(root, "docs"), ".md"),
  ];
  for (const file of mdFiles) {
    const rel = relative(root, file).replaceAll("\\", "/");
    issues.push(...inspectMarkdownContent(rel, readFileSync(file), dirname(file)));
  }

  const localeDir = join(root, "crates", "flowy-i18n", "locales");
  if (existsSync(localeDir)) {
    for (const name of readdirSync(localeDir).filter((n) => n.endsWith(".json")).sort()) {
      const rel = `flowy-i18n/locales/${name}`;
      try {
        issues.push(...inspectLocaleNaming(rel, JSON.parse(readFileSync(join(localeDir, name), "utf8"))));
      } catch (err) {
        issues.push(`${rel}: invalid JSON (${err.message})`);
      }
    }
  }
  return issues;
}

export function selfTest() {
  assert.deepEqual(inspectScrollUsage("flowy-ui/src/scroll.rs", "fn wrap() { div().overflow_y_scroll().scrollbar_width(px(10.0)); }"), []);
  assert.deepEqual(inspectScrollUsage("flowy-app/src/app/dock.rs", "fn list() { div().overflow_y_scroll(); }").length, 1);
  assert.deepEqual(inspectScrollUsage("flowy-app/src/app/dock.rs", "fn list() { div().scrollbar_width(px(6.0)); }").length, 1);
  assert.deepEqual(inspectScrollUsage("flowy-app/src/app/dock.rs", "#[cfg(test)]\nmod tests {\n fn t() { div().overflow_y_scroll(); }\n}"), []);
  assert.deepEqual(inspectScrollUsage("flowy-ui/src/overlays.rs", "fn a() { div().overflow_y_scroll(); }"), []);
  assert.equal(inspectScrollUsage("flowy-ui/src/overlays.rs", "fn a() { div().overflow_y_scroll(); div().overflow_y_scroll(); }").length, 1);

  const memExists = (p) => p.endsWith("ok.md");
  assert.deepEqual(inspectMarkdownContent("docs/a.md", Buffer.from("# Title\n[link](./ok.md)\n", "utf8"), "/repo/docs", memExists), []);
  assert.equal(inspectMarkdownContent("docs/a.md", Buffer.from([0xef, 0xbb, 0xbf, 0x23]), "/repo/docs", memExists).length, 1);
  assert.equal(inspectMarkdownContent("docs/a.md", Buffer.from("line1\r\nline2\n", "utf8"), "/repo/docs", memExists).length, 1);
  assert.equal(inspectMarkdownContent("docs/a.md", Buffer.from("```rs\nfn a() {}\n", "utf8"), "/repo/docs", memExists).length, 1);
  assert.equal(inspectMarkdownContent("docs/a.md", Buffer.from("[bad](./missing.md)\n", "utf8"), "/repo/docs", memExists).length, 1);
  assert.equal(inspectMarkdownContent("docs/a.md", Buffer.from("[abs](D:/foo/bar.md)\n", "utf8"), "/repo/docs", memExists).length, 1);
  assert.deepEqual(inspectMarkdownContent("docs/a.md", Buffer.from("```\n[skip](./missing.md)\n```\n`[inline](./missing.md)`\n", "utf8"), "/repo/docs", memExists), []);

  assert.deepEqual(inspectLocaleNaming("zh-CN.json", { "a11y.scroll_dock": "导航滚动区" }), []);
  assert.equal(inspectLocaleNaming("zh-CN.json", { "a11y.scrollDock": "bad" }).length, 1);
  console.log("Flowy 1.8 docs & scroll scanner: 15 fixture assertions passed");
}

const invokedPath = process.argv[1] ? resolve(process.argv[1]) : "";
if (import.meta.main || invokedPath === fileURLToPath(import.meta.url)) {
  if (process.argv.includes("--self-test")) {
    selfTest();
  } else {
    const issues = runChecks();
    if (issues.length) {
      console.error(issues.join("\n"));
      process.exitCode = 1;
    } else {
      console.log("Flowy 1.8 docs, ScrollArea, and a11y contracts passed");
    }
  }
}
