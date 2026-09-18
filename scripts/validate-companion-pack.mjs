#!/usr/bin/env bun
/**
 * Validate CompanionPack directories: explicit runtime.json, no extra files,
 * no path-guessed atlas rows. Mirrors nomifun_companion::presence::validate_pack_dir.
 *
 *   bun run validate:companion-pack
 *   bun run validate:companion-pack -- crates/backend/nomifun-companion/packs/builtin/mochi
 */
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const DEFAULT_ROOT = join(ROOT, 'crates/backend/nomifun-companion/packs');
const ALLOWED = new Set(['runtime.json', 'pack.manifest.json']);
const ACTIVITIES = new Set([
  'idle',
  'thinking',
  'busy',
  'awaiting_user',
  'review',
  'failed',
  'interacting',
]);
const INTENTS = new Set(['head', 'body', 'raise']);

function kebabSlug(part) {
  return (
    part.length > 0 &&
    !part.startsWith('-') &&
    !part.endsWith('-') &&
    !part.includes('--') &&
    /^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(part)
  );
}

function packIdOk(id) {
  if (typeof id !== 'string' || id.length === 0 || id.length > 80) return false;
  const parts = id.split('--');
  return parts.length <= 2 && parts.every(kebabSlug);
}

function validateHitArea(area, idx) {
  if (!area || typeof area !== 'object') return `hit_areas[${idx}] must be an object`;
  if (typeof area.id !== 'string' || area.id.trim() === '') return `hit_areas[${idx}].id is empty`;
  if (!INTENTS.has(area.intent)) return `hit_areas[${idx}].intent is invalid`;
  for (const key of ['x', 'y', 'w', 'h']) {
    if (typeof area[key] !== 'number' || !Number.isFinite(area[key])) {
      return `hit_areas[${idx}].${key} must be a finite number`;
    }
  }
  if (area.w <= 0 || area.h <= 0) return `hit_areas[${idx}] needs positive w/h`;
  if (area.x < 0 || area.y < 0 || area.x + area.w > 1.000001 || area.y + area.h > 1.000001) {
    return `hit_areas[${idx}] must lie inside the unit square`;
  }
  return null;
}

function validateRuntime(pack, dir) {
  if (pack.schema_version !== 1) return `unsupported schema_version ${pack.schema_version}`;
  if (!packIdOk(pack.id)) return `invalid pack id '${pack.id}'`;
  if (typeof pack.display_name !== 'string' || pack.display_name.trim() === '') {
    return 'display_name must not be empty';
  }
  const renderer = pack.renderer ?? 'css';
  if (renderer !== 'css' && renderer !== 'atlas') return `unknown renderer ${renderer}`;
  if (renderer === 'css' && pack.atlas) return 'css pack must not declare an atlas';
  if (renderer === 'atlas') {
    const atlas = pack.atlas;
    if (!atlas || typeof atlas.path !== 'string') return 'atlas pack requires atlas.path';
    if (atlas.path.includes('..') || atlas.path.includes('/') || atlas.path.includes('\\')) {
      return 'atlas.path must be a bare filename';
    }
    if (!atlas.path.endsWith('.webp')) return 'atlas.path must be a .webp file';
    if (!existsSync(join(dir, atlas.path))) return `missing atlas file ${atlas.path}`;
    if (!Array.isArray(atlas.states)) return 'atlas.states must be an array';
    const seen = new Set();
    for (const state of atlas.states) {
      if (!state?.id || seen.has(state.id)) return `duplicate or empty atlas state id`;
      seen.add(state.id);
      if (!state.frames) return `atlas state ${state.id} needs frames`;
    }
  }
  for (const [i, area] of (pack.hit_areas ?? []).entries()) {
    const err = validateHitArea(area, i);
    if (err) return err;
  }
  for (const [name, clip] of Object.entries(pack.clips ?? {})) {
    if (!name || !clip?.a || !clip?.b || !clip?.c) return `clip '${name}' has an empty phase`;
  }
  for (const [from, to] of Object.entries(pack.agent_bindings ?? {})) {
    if (!from || !ACTIVITIES.has(to)) return `agent_bindings['${from}'] is not a CompanionActivity`;
  }
  return null;
}

function extraFiles(dir, pack) {
  const extra = [];
  for (const name of readdirSync(dir)) {
    if (ALLOWED.has(name)) continue;
    if (pack.renderer === 'atlas' && pack.atlas?.path === name) continue;
    extra.push(name);
  }
  extra.sort();
  return extra;
}

function collectDirs(argv) {
  const explicit = argv.filter((arg) => arg !== '--');
  if (explicit.length) return explicit.map((p) => resolve(p));
  if (!existsSync(DEFAULT_ROOT)) return [];
  const found = [];
  const walk = (dir) => {
    for (const name of readdirSync(dir)) {
      const full = join(dir, name);
      if (!statSync(full).isDirectory()) continue;
      if (existsSync(join(full, 'runtime.json'))) found.push(full);
      else walk(full);
    }
  };
  walk(DEFAULT_ROOT);
  return found;
}

const dirs = collectDirs(process.argv.slice(2));
if (dirs.length === 0) {
  console.error('validate:companion-pack: no pack directories found');
  process.exit(1);
}

const failures = [];
for (const dir of dirs) {
  const runtimePath = join(dir, 'runtime.json');
  if (!existsSync(runtimePath)) {
    failures.push(`${dir}: missing runtime.json`);
    continue;
  }
  let pack;
  try {
    pack = JSON.parse(readFileSync(runtimePath, 'utf8'));
  } catch (error) {
    failures.push(`${dir}: ${error.message}`);
    continue;
  }
  const err = validateRuntime(pack, dir);
  if (err) {
    failures.push(`${dir}: ${err}`);
    continue;
  }
  const extra = extraFiles(dir, pack);
  if (extra.length) {
    failures.push(`${dir}: extra files ${extra.join(', ')}`);
  }
}

if (failures.length) {
  for (const failure of failures) console.error(failure);
  process.exit(1);
}

console.log(`validate:companion-pack: ${dirs.length} pack(s) ok`);
