#!/usr/bin/env bun
/** GPUI-specific source contracts. Run from any directory with Bun or Node. */
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
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

function inspect(path, source) {
  // Production code only. Fixtures in #[cfg(test)] modules are deliberately
  // isolated from runtime contracts and are checked by Cargo instead.
  const production = source.split(/#\[cfg\(test\)\]/, 1)[0];
  const code = codeOnly(production);
  const issues = [];
  const add = (offset, message) => issues.push(`${path}:${production.slice(0, offset).split("\n").length}: ${message}`);
  const ui = path.startsWith("flowy-ui/");
  if (ui) {
    for (const match of code.matchAll(/\bstatic\s+(?:mut\s+)?(?:DARK|ACCENT|DENSITY)\b|\bAtomic(?:Bool|U8)\b/g)) add(match.index, "theme state must belong to the window, not a process global");
  }
  for (const match of code.matchAll(/\b(?:theme|flowy_ui)::(?:install|canvas|dock|card|ink|muted|line|soft|composer|on_composer|composer_muted|danger|on_ink|accent|fill|on_fill|space|swatch|scrim|scrim_blur|modal_radius|modal_size)\s*\(/g)) add(match.index, "read the explicit ResolvedTheme snapshot instead of a legacy global token");
  for (const match of code.matchAll(/\bcredits\s*:\s*2_?500\b|\bDAILY_GRANT\b/g)) add(match.index, "production credits must come from CreditsViewState, never a fabricated default or split");
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
  return issues;
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
  console.log("UI contract scanner: 24 fixture assertions passed");
}

function rustFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? rustFiles(path) : entry.name.endsWith(".rs") ? [path] : [];
  });
}

if (process.argv.includes("--self-test")) {
  selfTest();
} else {
  const issues = ["flowy-ui", "flowy-app", "flowy-studio"].flatMap(crate =>
    rustFiles(join(root, "crates", crate, "src")).flatMap(path => inspect(relative(join(root, "crates"), path).replaceAll("\\", "/"), readFileSync(path, "utf8"))));
  issues.push(...localeIssues());
  if (issues.length) {
    console.error(issues.join("\n"));
    process.exitCode = 1;
  } else console.log("Flowy 1.8 UI contracts passed");
}
