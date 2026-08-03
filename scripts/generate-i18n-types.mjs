#!/usr/bin/env node
/**
 * generate-i18n-types.mjs — regenerate ui/src/renderer/services/i18n/i18n-keys.d.ts
 * from the en-US locale JSON files (source of truth).
 *
 * Usage:
 *   node scripts/generate-i18n-types.mjs           # write the d.ts
 *   node scripts/generate-i18n-types.mjs --check   # no write; exit 1 if the
 *                                                  # committed d.ts drifts from
 *                                                  # the locale key set
 *
 * No dependencies. Node >= 16.
 *
 * Rules (mirrors the historical generator output):
 * - Namespaces and their order come from locales/en-US/index.ts (runtime truth).
 * - Every locales/<code>/*.json MUST be imported and exported by that locale's
 *   index.ts; orphan JSON files are a hard error (they never reach i18next).
 * - en-US and zh-CN barrels must export the same namespaces in the same order
 *   with matching JSON filenames.
 * - Keys are the dot-flattened paths of every leaf value, prefixed with the
 *   namespace; arrays flatten to numeric indices (e.g. `a.list.0`).
 * - I18nKey union is sorted by UTF-16 code units; I18nModule keeps index.ts order.
 * - Output uses LF line endings (repo-wide `.gitattributes`: `* text=auto eol=lf`).
 */

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const i18nDir = path.join(repoRoot, 'ui', 'src', 'renderer', 'services', 'i18n');
const localesRoot = path.join(i18nDir, 'locales');
const localeDir = path.join(localesRoot, 'en-US');
const outFile = path.join(i18nDir, 'i18n-keys.d.ts');
const LOCALE_CODES = ['en-US', 'zh-CN'];

const checkMode = process.argv.includes('--check');

/**
 * Parse locales/<code>/index.ts: namespace export order + json file per namespace.
 * Hard-fails when a JSON file on disk is not exported by the barrel — that
 * drift is invisible to i18next at runtime and surfaces as raw key strings.
 */
function readNamespaces(code) {
  const dir = path.join(localesRoot, code);
  const indexPath = path.join(dir, 'index.ts');
  const src = fs.readFileSync(indexPath, 'utf8');

  const importMap = new Map(); // identifier -> json filename
  const importRe = /import\s+(\w+)\s+from\s+'\.\/([\w.-]+)\.json'/g;
  for (let m; (m = importRe.exec(src)); ) importMap.set(m[1], `${m[2]}.json`);

  const block = src.match(/export\s+default\s*\{([\s\S]*?)\}/);
  if (!block) throw new Error(`export default block not found in ${indexPath}`);
  const names = block[1]
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean);

  const namespaces = names.map((name) => {
    // supports shorthand (`common`) and aliased (`starOffice: starOffice`) entries
    const [exportName, ident = exportName] = name.split(':').map((s) => s.trim());
    const file = importMap.get(ident);
    if (!file) throw new Error(`namespace '${exportName}' in ${code}/index.ts has no matching JSON import`);
    return { name: exportName, file };
  });

  const referenced = new Set(namespaces.map((n) => n.file));
  const onDisk = fs.readdirSync(dir).filter((f) => f.endsWith('.json'));
  const orphans = onDisk.filter((f) => !referenced.has(f)).sort();
  const missingFiles = [...referenced].filter((f) => !onDisk.includes(f)).sort();

  if (orphans.length || missingFiles.length) {
    const lines = [`locale barrel drift in ${code}/index.ts:`];
    if (orphans.length) {
      lines.push(`  JSON on disk but not exported (${orphans.length}):`);
      for (const f of orphans) lines.push(`    - ${f}`);
    }
    if (missingFiles.length) {
      lines.push(`  exported but JSON missing (${missingFiles.length}):`);
      for (const f of missingFiles) lines.push(`    - ${f}`);
    }
    lines.push('  Fix: import and export every locales/<code>/*.json from index.ts.');
    throw new Error(lines.join('\n'));
  }

  return namespaces;
}

/** en-US is the type-generation source of truth; zh-CN must export the same modules. */
function assertLocaleBarrelParity(enNamespaces, zhNamespaces) {
  const enNames = enNamespaces.map((n) => n.name);
  const zhNames = zhNamespaces.map((n) => n.name);
  const enSet = new Set(enNames);
  const zhSet = new Set(zhNames);
  const onlyEn = enNames.filter((n) => !zhSet.has(n));
  const onlyZh = zhNames.filter((n) => !enSet.has(n));
  if (onlyEn.length || onlyZh.length) {
    const lines = ['locale barrel namespace mismatch between en-US and zh-CN:'];
    if (onlyEn.length) lines.push(`  only in en-US: ${onlyEn.join(', ')}`);
    if (onlyZh.length) lines.push(`  only in zh-CN: ${onlyZh.join(', ')}`);
    throw new Error(lines.join('\n'));
  }
  for (let i = 0; i < enNamespaces.length; i++) {
    if (enNamespaces[i].file !== zhNamespaces[i].file || enNamespaces[i].name !== zhNamespaces[i].name) {
      throw new Error(
        `locale barrel order/file mismatch at index ${i}: ` +
          `en-US=${enNamespaces[i].name}:${enNamespaces[i].file} ` +
          `zh-CN=${zhNamespaces[i].name}:${zhNamespaces[i].file}`,
      );
    }
  }
}

/** Dot-flatten a JSON value into `out`; arrays become numeric segments. */
function flatten(value, prefix, out) {
  if (Array.isArray(value)) {
    value.forEach((v, i) => flatten(v, `${prefix}.${i}`, out));
  } else if (value !== null && typeof value === 'object') {
    for (const [k, v] of Object.entries(value)) flatten(v, `${prefix}.${k}`, out);
  } else {
    out.push(prefix);
  }
}

function collectKeys(namespaces) {
  const keys = [];
  for (const { name, file } of namespaces) {
    const json = JSON.parse(fs.readFileSync(path.join(localeDir, file), 'utf8'));
    flatten(json, name, keys);
  }
  // Some locale files (e.g. settings.json) contain both a flat dotted key
  // ("assistant.botToken") and a nested object ("assistant": { "botToken" })
  // that flatten to the same path. The union type lists each key once, so we
  // dedupe — but surface the collisions as a lint warning.
  const seen = new Set();
  const dupes = new Set();
  for (const k of keys) (seen.has(k) ? dupes : seen).add(k);
  if (dupes.size) {
    process.stderr.write(
      `warning: ${dupes.size} flattened key collisions (flat dotted key + nested object), deduped:\n  ${[...dupes].join('\n  ')}\n`,
    );
  }
  return [...seen].sort(); // UTF-16 code unit order, matches historical output
}

const quote = (s) => `'${s.replace(/\\/g, '\\\\').replace(/'/g, "\\'")}'`;
const union = (items) => items.map((k) => `  | ${quote(k)}`).join('\n');

function render(namespaces, keys) {
  return [
    '/* eslint-disable */',
    '/**',
    ' * AUTO-GENERATED FILE - DO NOT EDIT',
    ' * Generated by scripts/generate-i18n-types.mjs',
    ' */',
    '',
    'export type I18nKey =',
    `${union(keys)};`,
    '',
    'export type I18nModule =',
    `${union(namespaces.map((n) => n.name))};`,
    '',
  ].join('\n');
}

const normalize = (s) => s.replace(/\r\n/g, '\n');

function main() {
  const namespacesByLocale = Object.fromEntries(LOCALE_CODES.map((code) => [code, readNamespaces(code)]));
  assertLocaleBarrelParity(namespacesByLocale['en-US'], namespacesByLocale['zh-CN']);
  const namespaces = namespacesByLocale['en-US'];
  const keys = collectKeys(namespaces);
  const generated = render(namespaces, keys);

  const existing = fs.existsSync(outFile) ? normalize(fs.readFileSync(outFile, 'utf8')) : null;

  if (checkMode) {
    if (existing === generated) {
      console.log(`i18n-keys.d.ts is up to date (${keys.length} keys, ${namespaces.length} modules)`);
      return;
    }
    // Report drift at key granularity, then fall back to a text-level hint.
    const extractKeys = (text) => {
      const section = text.split('export type I18nModule')[0];
      return new Set([...section.matchAll(/\|\s+'((?:[^'\\]|\\.)*)'/g)].map((m) => m[1]));
    };
    const oldKeys = existing ? extractKeys(existing) : new Set();
    const newKeys = new Set(keys);
    const missing = keys.filter((k) => !oldKeys.has(k)); // in locales, not in d.ts
    const stale = [...oldKeys].filter((k) => !newKeys.has(k)); // in d.ts, not in locales
    if (missing.length) console.error(`missing from d.ts (${missing.length}):\n  ${missing.join('\n  ')}`);
    if (stale.length) console.error(`stale in d.ts (${stale.length}):\n  ${stale.join('\n  ')}`);
    if (!missing.length && !stale.length) console.error('key sets match but file text differs (ordering/header/EOL)');
    console.error('\ni18n-keys.d.ts is out of date — run: node scripts/generate-i18n-types.mjs');
    process.exitCode = 1;
    return;
  }

  if (existing === generated) {
    console.log(`i18n-keys.d.ts already up to date (${keys.length} keys)`);
    return;
  }
  fs.writeFileSync(outFile, generated, 'utf8');
  console.log(`wrote ${path.relative(repoRoot, outFile)} (${keys.length} keys, ${namespaces.length} modules)`);
}

main();
