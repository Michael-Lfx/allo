#!/usr/bin/env bun
/** GPUI UI and workspace source contracts. Run from any directory with Bun or Node. */
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, posix, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import assert from "node:assert/strict";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

// These panels consume a click to stop propagation or dismiss a scrim. They
// are presentation infrastructure, not operation controls. Entries are scoped
// to both source file and stable element id.
const infrastructure = new Map([
  ["flowy-app/src/app/settings.rs", new Set(["settings-layer", "settings-panel"])],
  ["flowy-app/src/app/flex.rs", new Set(["flex-layer", "flex-dialog"])],
  ["flowy-app/src/app/gallery.rs", new Set(["ideas-layer", "ideas-panel", "idea-layer", "idea-panel"])],
  ["flowy-app/src/app/invite.rs", new Set(["invite-layer", "invite-card"])],
  ["flowy-app/src/app/login.rs", new Set(["legal-layer", "legal-dialog"])],
  ["flowy-app/src/app/more.rs", new Set(["more-layer", "more-flyout"])],
]);

// Illustration/media surfaces intentionally retain authored asset colors.
// New ordinary UI colors belong in ResolvedTheme, never in this allowlist.
const authoredColors = new Map([
  ["flowy-app/src/app/gallery.rs", new Set(["idea_stage", "transport", "transport_hit", "volume_mark", "full_mark", "poster", "thumb"])],
  ["flowy-app/src/app/invite.rs", new Set(["social_row", "envelope"])],
  ["flowy-app/src/app/login.rs", new Set(["wire_card", "wire_dot", "wire_line"])],
  // Fixed skill cover/hero artwork and its lettermark; action controls in the
  // same file must still use theme tokens and shared interaction primitives.
  ["flowy-app/src/app/skills.rs", new Set(["skill_cover", "skill_hero", "skill_mark"])],
]);

/** Preserve offsets while hiding Rust comments and literal contents. */
function codeOnly(source) {
  const out = source.split("");
  const blank = (a, b) => { for (let j = a; j < b; j++) if (out[j] !== "\n") out[j] = " "; };
  for (let i = 0; i < source.length;) {
    if (source.startsWith("//", i)) {
      const end = source.indexOf("\n", i);
      const next = end < 0 ? source.length : end;
      blank(i, next); i = next;
    } else if (source.startsWith("/*", i)) {
      const start = i; let depth = 1; i += 2;
      while (i < source.length && depth) {
        if (source.startsWith("/*", i)) { depth++; i += 2; }
        else if (source.startsWith("*/", i)) { depth--; i += 2; }
        else i++;
      }
      blank(start, i);
    } else {
      const raw = source.slice(i).match(/^(?:br|r)(#*)"/);
      if (raw) {
        const endMark = `"${raw[1]}`;
        const end = source.indexOf(endMark, i + raw[0].length);
        const next = end < 0 ? source.length : end + endMark.length;
        blank(i, next); i = next;
      } else if (source[i] === '"') {
        const start = i++;
        while (i < source.length) {
          if (source[i] === "\\") i += 2;
          else if (source[i++] === '"') break;
        }
        blank(start, i);
      } else if (source[i] === "'" && /^'(?:[^'\\]|\\(?:.|u\{[^}]+\}))'/.test(source.slice(i))) {
        const literal = source.slice(i).match(/^'(?:[^'\\]|\\(?:.|u\{[^}]+\}))'/)[0];
        blank(i, i + literal.length); i += literal.length;
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

function functions(code) {
  const ranges = [];
  for (const match of code.matchAll(/\bfn\s+(\w+)/g)) {
    const start = code.indexOf("{", match.index);
    if (start >= 0) ranges.push({ name: match[1], start, end: closing(code, start) });
  }
  return ranges;
}

/** Scan only each div's own fluent chain, not nested children or closures. */
function rawClickableDivs(source, code) {
  const results = [];
  for (const match of code.matchAll(/\bdiv\s*\(\s*\)/g)) {
    let cursor = match.index + match[0].length;
    let id; let clicked = false;
    while (cursor < code.length) {
      const method = code.slice(cursor).match(/^\s*\.\s*(\w+)\s*\(/);
      if (!method) break;
      const open = cursor + method[0].lastIndexOf("(");
      const end = closing(code, open);
      if (method[1] === "id") id = source.slice(open + 1, end).trim().match(/^"([^"\n]+)"$/)?.[1];
      if (method[1] === "on_click") clicked = true;
      cursor = end + 1;
    }
    if (clicked) results.push({ offset: match.index, id });
  }
  return results;
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

/** Blank only #[cfg(test)]-gated items/fields/blocks while preserving newlines and offsets. */
function stripTestCode(source) {
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

const wildcardBarrelFiles = new Set([
  "flowy-app/src/lib.rs",
  "flowy-studio/src/motion.rs",
  "flowy-ui/src/lib.rs",
]);

function scanProduction(path, source, visitor, prepared = { production: stripTestCode(source) }) {
  const code = prepared.code ??= codeOnly(prepared.production);
  const issues = [];
  const add = (offset, message) => issues.push(`${path}:${prepared.production.slice(0, offset).split("\n").length}: ${message}`);
  visitor(prepared.production, code, add);
  return issues;
}

function inspect(path, source, prepared) {
  // Production code only. Fixtures in #[cfg(test)] items are deliberately
  // isolated from runtime contracts and are checked by Cargo instead.
  return scanProduction(path, source, (production, code, add) => {
    const ui = path.startsWith("flowy-ui/");
    if (ui) {
      for (const match of code.matchAll(/\bstatic\s+(?:mut\s+)?(?:DARK|ACCENT|DENSITY)\b|\bAtomic(?:Bool|U8)\b/g)) add(match.index, "theme state must belong to the window, not a process global");
    }
    for (const match of code.matchAll(/\b(?:theme|flowy_ui)::(?:install|canvas|dock|card|ink|muted|line|soft|composer|on_composer|composer_muted|danger|on_ink|accent|fill|on_fill|space|swatch|scrim|scrim_blur|modal_radius|modal_size)\s*\(/g)) add(match.index, "read the explicit ResolvedTheme snapshot instead of a legacy global token");
    for (const match of code.matchAll(/\bcredits\s*:\s*2_?500\b|\bDAILY_GRANT\b/g)) add(match.index, "production credits must come from CreditsViewState, never a fabricated default or split");
    for (const match of code.matchAll(/\bRole::MenuItemRadio\b/g)) add(match.index, "Role::MenuItemRadio strips Windows UIA InvokePattern; keep clickable controls as Button");
    for (const match of code.matchAll(/\.aria_expanded\s*\(/g)) add(match.index, ".aria_expanded strips Windows UIA InvokePattern in gpui-pre 0.3.14; convey state visually");
    if (!ui) {
      const ranges = functions(code);
      for (const match of code.matchAll(/\b(?:rgb|rgba)\s*\(/g)) {
        const name = ranges.find(range => range.start <= match.index && range.end >= match.index)?.name;
        if (!authoredColors.get(path)?.has(name)) add(match.index, "raw UI color outside an explicitly named illustration/media function");
      }
      for (const item of rawClickableDivs(production, code)) {
        if (!item.id || !infrastructure.get(path)?.has(item.id)) add(item.offset, `operation controls must use flowy_ui primitives (raw div id=${item.id ?? "dynamic/absent"})`);
      }
      for (const range of ranges) {
        const body = code.slice(range.start, range.end);
        if (/menu|flyout|popover|model_info|credit_card/.test(range.name) && /\.absolute\s*\(/.test(body) && /\.(?:top|left|right|bottom)\s*\(\s*px\s*\(/.test(body)) add(range.start, "floating menus must use measured anchors, not fixed page coordinates");
      }
    }
  }, prepared);
}

/** Guard all active crates' production `src/` against unapproved wildcard imports. */
function inspectWildcards(path, source, prepared) {
  return scanProduction(path, source, (_production, code, add) => {
    for (const match of code.matchAll(/\b(?:(pub(?:\s*\([^)]*\))?)\s+)?use\s+([^;]+);/g)) {
      const [, pubVis, target] = match;
      if (!/\*/.test(target)) continue;
      const normalized = target.replace(/\s+/g, "");
      const isGpuiPreludeOnly =
        normalized === "gpui::prelude::*" ||
        (normalized.startsWith("gpui::{") && !/\*/.test(normalized.replace(/(^|[{,])prelude::\*(?=[,}])/g, "$1")));
      if (isGpuiPreludeOnly) continue;
      if (pubVis === "pub" && wildcardBarrelFiles.has(path)) continue;
      add(match.index, `avoid wildcard glob imports (${target.trim()}) in production modules`);
    }
  }, prepared);
}

/** Parses out-of-line `mod name;` and `#[path = "..."] mod name;` declarations. */
function parseModDecls(filePath, source) {
  const masked = codeOnly(source);
  const decls = [];
  const attrModRe = /((?:#\s*\[[^\]]*\]\s*)*)(?:\bpub(?:\s*\([^)]*\))?\s+)?\bmod\s+(\w+)\s*;/g;
  for (const match of masked.matchAll(attrModRe)) {
    const attrRaw = source.slice(match.index, match.index + match[1].length);
    const attrMasked = match[1];
    const isCfgTest = /#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]/.test(attrMasked);
    const pathMatch = attrRaw.match(/#\s*\[\s*path\s*=\s*"([^"\n]+)"\s*\]/);
    const modName = match[2];
    const dir = posix.dirname(filePath);
    const base = posix.basename(filePath);
    const subDir = (base === "lib.rs" || base === "main.rs" || base === "mod.rs" || base === "build.rs")
      ? dir
      : posix.join(dir, base.replace(/\.rs$/, ""));
    const targets = pathMatch
      ? [posix.normalize(posix.join(dir, pathMatch[1]))]
      : [posix.join(subDir, `${modName}.rs`), posix.join(subDir, modName, "mod.rs")];
    decls.push({ isCfgTest, targets });
  }
  return decls;
}

/** Computes `.rs` files in a crate reachable only under `#[cfg(test)]`. */
function resolveTestOnlyPaths(crateName, entries) {
  const byPath = new Map(entries.map(entry => [entry.relPath, entry.source]));
  const parsed = new Map(entries.map(entry => [entry.relPath, parseModDecls(entry.relPath, entry.source)]));

  const reachableProd = new Set();
  const prodQueue = [
    `${crateName}/src/lib.rs`,
    `${crateName}/src/main.rs`,
    `${crateName}/build.rs`,
    ...entries.map(e => e.relPath).filter(p => p.startsWith(`${crateName}/src/bin/`) && p.endsWith(".rs")),
  ].filter(p => byPath.has(p));
  for (const p of prodQueue) reachableProd.add(p);

  while (prodQueue.length) {
    const current = prodQueue.shift();
    for (const decl of parsed.get(current) ?? []) {
      if (decl.isCfgTest) continue;
      for (const target of decl.targets) {
        if (byPath.has(target) && !reachableProd.has(target)) {
          reachableProd.add(target);
          prodQueue.push(target);
        }
      }
    }
  }

  const testOnly = new Set();
  const testQueue = [];
  for (const decls of parsed.values()) {
    for (const decl of decls) {
      if (!decl.isCfgTest) continue;
      for (const target of decl.targets) {
        if (byPath.has(target) && !reachableProd.has(target) && !testOnly.has(target)) {
          testOnly.add(target);
          testQueue.push(target);
        }
      }
    }
  }
  while (testQueue.length) {
    const current = testQueue.shift();
    for (const decl of parsed.get(current) ?? []) {
      for (const target of decl.targets) {
        if (byPath.has(target) && !reachableProd.has(target) && !testOnly.has(target)) {
          testOnly.add(target);
          testQueue.push(target);
        }
      }
    }
  }
  return testOnly;
}

/** Guard all active crates' production `src/` against `.unwrap()` panics. */
function inspectUnwraps(path, source, prepared) {
  return scanProduction(path, source, (_production, code, add) => {
    for (const match of code.matchAll(/\.\s*unwrap\s*\(/g)) {
      add(match.index, "production code must handle errors instead of calling .unwrap()");
    }
  }, prepared);
}

/** Flat locale contracts run without compiling the native workspace. */
function compareLocale(path, messages, base) {
  const issues = [];
  if (messages === null || Array.isArray(messages) || typeof messages !== "object") {
    return [`${path}: locale dictionary must be an object`];
  }
  const expected = new Set(Object.keys(base));
  for (const [key, value] of Object.entries(messages)) {
    if (!expected.has(key)) issues.push(`${path}: unexpected locale key ${key}`);
    if (typeof value !== "string" || !value.trim()) issues.push(`${path}: locale key ${key} must contain a non-empty string`);
  }
  for (const key of expected) {
    if (!Object.hasOwn(messages, key)) issues.push(`${path}: missing locale key ${key}`);
  }
  return issues;
}

function localeIssues() {
  const directory = join(root, "crates", "flowy-i18n", "locales");
  const issues = [];
  let base;
  try {
    base = JSON.parse(readFileSync(join(directory, "zh-CN.json"), "utf8"));
  } catch (error) {
    return [`flowy-i18n/locales/zh-CN.json: invalid locale JSON: ${error.message}`];
  }
  for (const name of readdirSync(directory).filter(name => name.endsWith(".json"))) {
    const path = `flowy-i18n/locales/${name}`;
    try {
      issues.push(...compareLocale(path, JSON.parse(readFileSync(join(directory, name), "utf8")), base));
    } catch (error) {
      issues.push(`${path}: invalid locale JSON: ${error.message}`);
    }
  }
  return issues;
}

function selfTest() {
  const source = `fn example() { div().id("outer").child(interactive("inner", label, theme).on_click(callback)); }`;
  assert.equal(inspect("flowy-app/src/example.rs", source).length, 0);
  assert.equal(inspect("flowy-app/src/example.rs", `fn example() { div().id("bad").child(div()).on_click(callback); }`).length, 1);
  assert.equal(inspect("flowy-app/src/example.rs", `fn example() { div().id("outer").child(div().id("inner").on_click(callback)); }`).length, 1);
  assert.equal(inspect("flowy-app/src/app/settings.rs", `fn layer() { div().id("settings-layer").on_click(callback); }`).length, 0);
  assert.equal(inspect("flowy-app/src/example.rs", `// theme::install(false)\nfn a() { let x = r#"rgb(0); div().on_click(c)"#; }`).length, 0);
  assert.equal(inspect("flowy-app/src/example.rs", `fn a() { rgb(0xffffff); }`).length, 1);
  assert.equal(inspect("flowy-app/src/app/gallery.rs", `fn poster() { rgb(tint); }`).length, 0);
  for (const name of ["skill_cover", "skill_hero", "skill_mark"]) {
    assert.deepEqual(inspect("flowy-app/src/app/skills.rs", `fn ${name}() { rgb(tint); rgba(tint); }`), [], `${name} retains authored artwork colors`);
  }
  assert.deepEqual(inspect("flowy-app/src/app/skills.rs", `fn skill_cover() { rgb(tint); } fn skills_create_button() { rgb(0xffffff); }`), ["flowy-app/src/app/skills.rs:1: raw UI color outside an explicitly named illustration/media function"], "ordinary Skills controls still require theme tokens");
  assert.deepEqual(inspect("flowy-app/src/example.rs", `fn skill_cover() { rgb(tint); }`), ["flowy-app/src/example.rs:1: raw UI color outside an explicitly named illustration/media function"], "artwork color exceptions are scoped to the exact source file");
  assert.deepEqual(inspect("flowy-app/src/app/skills.rs", `fn skill_cover() { div().bg(rgb(tint)).on_click(callback); }`), ["flowy-app/src/app/skills.rs:1: operation controls must use flowy_ui primitives (raw div id=dynamic/absent)"], "artwork color exceptions do not exempt raw operation controls");
  assert.equal(inspect("flowy-app/src/example.rs", `fn a() { theme::canvas(); }`).length, 1);
  assert.equal(inspect("flowy-studio/src/panel.rs", `fn a() { State { credits: 2500 } }`).length, 1);
  assert.equal(inspect("flowy-ui/src/theme.rs", `static DARK: AtomicBool = AtomicBool::new(false);`).length, 3);
  assert.equal(inspect("flowy-ui/src/theme.rs", `struct ThemeState { dark: bool }`).length, 0);
  assert.equal(inspect("flowy-app/src/example.rs", `fn theme_menu() { div().absolute().top(px(188.)); }`).length, 1);
  assert.equal(inspect("flowy-app/src/example.rs", `fn decoration() { div().absolute().top(px(8.)); }`).length, 0);
  assert.deepEqual(compareLocale("en.json", { save: "Save" }, { save: "保存" }), []);
  assert.match(compareLocale("en.json", {}, { save: "保存" })[0], /missing locale key save/);
  assert.match(compareLocale("en.json", { save: "Save", other: "Other" }, { save: "保存" })[0], /unexpected locale key other/);
  assert.match(compareLocale("en.json", { save: " " }, { save: "保存" })[0], /non-empty string/);
  assert.match(compareLocale("en.json", [], { save: "保存" })[0], /must be an object/);
  assert.deepEqual(inspect("flowy-app/src/example.rs", "#[cfg(test)]\nmod tests;\nfn bad() {\n    rgb(0xffffff);\n}"), ["flowy-app/src/example.rs:4: raw UI color outside an explicitly named illustration/media function"]);
  assert.deepEqual(inspect("flowy-app/src/example.rs", "#[cfg(test)]\nuse foo::{rgb, Bar};\nfn bad() { rgb(0xffffff); }"), ["flowy-app/src/example.rs:3: raw UI color outside an explicitly named illustration/media function"]);
  assert.deepEqual(inspect("flowy-app/src/example.rs", "#[cfg(test)]\n#[must_use]\npub(super) fn helper() { rgb(0); }\nfn bad() { rgb(0xffffff); }"), ["flowy-app/src/example.rs:4: raw UI color outside an explicitly named illustration/media function"]);
  assert.deepEqual(inspect("flowy-app/src/example.rs", "struct S {\n    #[cfg(test)]\n    rendered: usize,\n}\nfn bad() { rgb(0xffffff); }"), ["flowy-app/src/example.rs:5: raw UI color outside an explicitly named illustration/media function"]);
  assert.deepEqual(inspect("flowy-app/src/example.rs", "fn render() {\n    #[cfg(test)]\n    { rgb(0); }\n    #[cfg(test)]\n    let link = link.debug_selector(move || { rgb(0) });\n    div().on_click(cb);\n}"), ["flowy-app/src/example.rs:6: operation controls must use flowy_ui primitives (raw div id=dynamic/absent)"]);
  assert.deepEqual(inspect("flowy-app/src/example.rs", "// #[cfg(test)]\nlet s = \"#[cfg(test)]\";\nfn bad() { rgb(0xffffff); }"), ["flowy-app/src/example.rs:3: raw UI color outside an explicitly named illustration/media function"]);
  assert.deepEqual(inspect("flowy-app/src/example.rs", "#[cfg(test)]\nmod tests {\n    #[cfg(test)]\n    fn nested() { rgb(0); }\n}\nfn bad() { theme::canvas(); }"), ["flowy-app/src/example.rs:6: read the explicit ResolvedTheme snapshot instead of a legacy global token"]);
  assert.deepEqual(inspect("flowy-app/src/example.rs", "struct Tail {\n    #[cfg(test)]\n    only: usize\n}\nfn bad() { rgb(0xffffff); }"), ["flowy-app/src/example.rs:5: raw UI color outside an explicitly named illustration/media function"]);
  assert.deepEqual(inspectUnwraps("flowy-agent/src/lib.rs", "fn prod() {\n    let _ = x.unwrap();\n}"), ["flowy-agent/src/lib.rs:2: production code must handle errors instead of calling .unwrap()"]);
  assert.deepEqual(inspectUnwraps("flowy-agent/src/lib.rs", "fn prod() {\n    let _ = x\n        .unwrap();\n}"), ["flowy-agent/src/lib.rs:3: production code must handle errors instead of calling .unwrap()"]);
  assert.deepEqual(inspectUnwraps("flowy-agent/src/lib.rs", "fn prod() { let _ = x.unwrap_or(0); let _ = y.unwrap_or_else(|| 1); let _ = z.unwrap_or_default(); let _ = w.expect(\"ok\"); }"), []);
  assert.deepEqual(inspectUnwraps("flowy-agent/src/lib.rs", "#[cfg(test)]\nfn helper() { x.unwrap(); }\nfn prod() {\n    y.unwrap();\n}"), ["flowy-agent/src/lib.rs:4: production code must handle errors instead of calling .unwrap()"]);
  assert.deepEqual(inspectUnwraps("flowy-net/src/proxy.rs", "#[cfg(any(test, target_os = \"linux\"))]\nfn prod_on_linux() { x.unwrap(); }"), ["flowy-net/src/proxy.rs:2: production code must handle errors instead of calling .unwrap()"]);
  assert.deepEqual(inspectUnwraps("flowy-core/src/lib.rs", "// #[cfg(test)]\nlet s = \"#[cfg(test)]\";\nfn prod() { x.unwrap(); }"), ["flowy-core/src/lib.rs:3: production code must handle errors instead of calling .unwrap()"]);
  const testOnlyTree = resolveTestOnlyPaths("demo", [
    { relPath: "demo/src/lib.rs", source: "#[cfg(test)]\nmod tests;\n#[cfg(test)]\n#[path = \"delivery_tests.rs\"]\nmod delivery;\n#[cfg(test)]\nmod brand_manifest;\n#[cfg(any(test, target_os = \"linux\"))]\nmod platform_helper;\nmod prod;" },
    { relPath: "demo/build.rs", source: "#[path = \"src/brand_manifest.rs\"]\nmod brand_manifest;" },
    { relPath: "demo/src/prod.rs", source: "fn ok() {}" },
    { relPath: "demo/src/platform_helper.rs", source: "fn p() {}" },
    { relPath: "demo/src/brand_manifest.rs", source: "fn b() {}" },
    { relPath: "demo/src/delivery_tests.rs", source: "fn dt() { x.unwrap(); }" },
    { relPath: "demo/src/tests/mod.rs", source: "mod helpers;" },
    { relPath: "demo/src/tests/helpers.rs", source: "fn h() { x.unwrap(); }" },
  ]);
  assert.equal(testOnlyTree.has("demo/src/tests/mod.rs") && testOnlyTree.has("demo/src/tests/helpers.rs"), true);
  assert.equal(testOnlyTree.has("demo/src/delivery_tests.rs"), true);
  assert.equal(testOnlyTree.has("demo/src/brand_manifest.rs"), false);
  assert.equal(testOnlyTree.has("demo/src/platform_helper.rs") || testOnlyTree.has("demo/src/prod.rs"), false);
  assert.deepEqual(inspect("flowy-app/src/example.rs", "fn item() { interactive(\"id\", \"l\", &t).role(Role::MenuItemRadio); }"), ["flowy-app/src/example.rs:1: Role::MenuItemRadio strips Windows UIA InvokePattern; keep clickable controls as Button"]);
  assert.deepEqual(inspect("flowy-ui/src/controls.rs", "fn item() { button().aria_expanded(true); }"), ["flowy-ui/src/controls.rs:1: .aria_expanded strips Windows UIA InvokePattern in gpui-pre 0.3.14; convey state visually"]);
  assert.deepEqual(inspectWildcards("flowy-studio/src/panel/body.rs", "use super::*;"), ["flowy-studio/src/panel/body.rs:1: avoid wildcard glob imports (super::*) in production modules"]);
  assert.deepEqual(inspectWildcards("flowy-agent/src/handle/mod.rs", "use gpui::{div, *};"), ["flowy-agent/src/handle/mod.rs:1: avoid wildcard glob imports (gpui::{div, *}) in production modules"]);
  assert.deepEqual(inspectWildcards("flowy-app/src/lib.rs", "pub(crate) use crate::brand::*;"), ["flowy-app/src/lib.rs:1: avoid wildcard glob imports (crate::brand::*) in production modules"]);
  assert.deepEqual(inspectWildcards("flowy-studio/src/automation_page.rs", "use gpui::prelude::*;"), []);
  assert.deepEqual(inspectWildcards("flowy-app/src/app/mod.rs", "use gpui::{Context, div, prelude::*, px};"), []);
  assert.deepEqual(inspectWildcards("flowy-ui/src/lib.rs", "pub use context::*;"), []);
  assert.deepEqual(inspectWildcards("flowy-agent/src/foo.rs", "#[cfg(test)]\nmod tests;\nuse super::*;"), ["flowy-agent/src/foo.rs:3: avoid wildcard glob imports (super::*) in production modules"]);
  assert.deepEqual(inspectWildcards("flowy-agent/src/foo.rs", "#[cfg(test)]\nmod tests {\n    use super::*;\n}"), []);
  console.log("UI contract scanner: 52 fixture assertions passed");
}

function rustFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? rustFiles(path) : entry.name.endsWith(".rs") ? [path] : [];
  });
}

const uiCrates = new Set(["flowy-ui", "flowy-app", "flowy-studio"]);

function crateSourceIssues(crateName) {
  const crateDir = join(root, "crates", crateName);
  const srcDir = join(crateDir, "src");
  if (!existsSync(srcDir)) return [];
  const srcFiles = rustFiles(srcDir);
  const buildRs = join(crateDir, "build.rs");
  const allFiles = existsSync(buildRs) ? [...srcFiles, buildRs] : srcFiles;
  const entries = allFiles.map(path => ({
    relPath: relative(join(root, "crates"), path).replaceAll("\\", "/"),
    source: readFileSync(path, "utf8"),
  }));
  const testOnly = resolveTestOnlyPaths(crateName, entries);
  const checkUi = uiCrates.has(crateName);
  return srcFiles.flatMap(path => {
    const relPath = relative(join(root, "crates"), path).replaceAll("\\", "/");
    if (testOnly.has(relPath)) return [];
    const source = readFileSync(path, "utf8");
    const prepared = { production: stripTestCode(source) };
    return [
      ...(checkUi ? inspect(relPath, source, prepared) : []),
      ...inspectWildcards(relPath, source, prepared),
      ...inspectUnwraps(relPath, source, prepared),
    ];
  });
}

if (process.argv.includes("--self-test")) {
  selfTest();
} else {
  const crates = readdirSync(join(root, "crates"), { withFileTypes: true })
    .filter(entry => entry.isDirectory())
    .map(entry => entry.name)
    .sort();
  const issues = [
    ...crates.flatMap(crateSourceIssues),
    ...localeIssues(),
  ];
  if (issues.length) {
    console.error(issues.join("\n"));
    process.exitCode = 1;
  } else console.log("Flowy 1.8 UI contracts passed");
}
