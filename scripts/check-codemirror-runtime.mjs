#!/usr/bin/env bun
/**
 * Verify that the CodeMirror runtime closure resolves through the versions
 * pinned by the UI workspace. Vite's `resolve.dedupe` is part of the proof:
 * nested copies may exist in node_modules, but they must not be eligible to
 * become a second runtime instance in the UI bundle.
 */
import { existsSync, readdirSync, readFileSync, realpathSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const UI_DIR = join(ROOT, 'ui');
const CORE_PACKAGES = [
  '@codemirror/state',
  '@codemirror/view',
  '@codemirror/language',
  '@codemirror/commands',
  '@lezer/common',
];
const RUNTIME_ROOTS = [...CORE_PACKAGES, '@uiw/react-codemirror', 'codemirror'];

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'));
}

function packageJsonPath(packageDir) {
  return join(packageDir, 'package.json');
}

function packageNameParts(name) {
  const parts = name.split('/');
  return parts[0].startsWith('@') ? [parts[0], parts[1]] : [parts[0]];
}

function resolveFrom(packageDir, dependency) {
  let cursor = packageDir;
  const [scope, name] = packageNameParts(dependency);
  while (true) {
    const candidate = scope.startsWith('@')
      ? join(cursor, 'node_modules', scope, name)
      : join(cursor, 'node_modules', scope);
    if (existsSync(packageJsonPath(candidate))) return realpathSync(candidate);
    const parent = dirname(cursor);
    if (parent === cursor) return null;
    cursor = parent;
  }
}

function collectNestedPackageDirs(nodeModulesDir, packageName, output) {
  if (!existsSync(nodeModulesDir)) return;
  let entries;
  try {
    entries = readdirSync(nodeModulesDir, { withFileTypes: true });
  } catch {
    return;
  }
  const [scope, name] = packageNameParts(packageName);
  for (const entry of entries) {
    if (!entry.isDirectory()) continue;
    const candidate = join(nodeModulesDir, entry.name);
    if (scope.startsWith('@')) {
      if (entry.name !== scope) {
        collectNestedPackageDirs(join(candidate, 'node_modules'), packageName, output);
        continue;
      }
      const scopedCandidate = join(candidate, name);
      if (existsSync(packageJsonPath(scopedCandidate))) {
        output.add(realpathSync(scopedCandidate));
        collectNestedPackageDirs(join(scopedCandidate, 'node_modules'), packageName, output);
      }
      continue;
    }
    if (entry.name === name && existsSync(packageJsonPath(candidate))) {
      output.add(realpathSync(candidate));
      collectNestedPackageDirs(join(candidate, 'node_modules'), packageName, output);
    } else {
      collectNestedPackageDirs(join(candidate, 'node_modules'), packageName, output);
    }
  }
}

function findAllInstalledInstances(packageName) {
  const instances = new Set();
  const root = join(UI_DIR, 'node_modules');
  const direct = resolveFrom(UI_DIR, packageName);
  if (direct) instances.add(direct);
  collectNestedPackageDirs(root, packageName, instances);
  return [...instances];
}

function isRuntimeDependency(name) {
  return name === 'codemirror' || name.startsWith('@codemirror/') || name.startsWith('@lezer/') || name.startsWith('@uiw/');
}

function collectRuntimeClosure() {
  const seen = new Set();
  const queue = RUNTIME_ROOTS.map((name) => ({ name, dir: resolveFrom(UI_DIR, name) })).filter((item) => item.dir);
  const instances = new Map(CORE_PACKAGES.map((name) => [name, new Set()]));
  while (queue.length) {
    const current = queue.shift();
    if (!current?.dir || seen.has(current.dir)) continue;
    seen.add(current.dir);
    let manifest;
    try {
      manifest = readJson(packageJsonPath(current.dir));
    } catch {
      continue;
    }
    if (instances.has(manifest.name)) instances.get(manifest.name).add(current.dir);
    const dependencies = { ...manifest.dependencies, ...manifest.optionalDependencies };
    for (const dependency of Object.keys(dependencies)) {
      if (!isRuntimeDependency(dependency)) continue;
      const dir = resolveFrom(current.dir, dependency);
      if (dir) queue.push({ name: dependency, dir });
    }
  }
  return instances;
}

function sourceFiles(dir) {
  const files = [];
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return files;
  }
  for (const entry of entries) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) files.push(...sourceFiles(path));
    else if (/\.(?:ts|tsx|js|jsx)$/.test(entry.name)) files.push(path);
  }
  return files;
}

function findBusinessImports() {
  const seam = join(UI_DIR, 'src', 'renderer', 'components', 'editors', 'CodeMirrorEditor.tsx');
  const violations = [];
  for (const path of sourceFiles(join(UI_DIR, 'src', 'renderer'))) {
    if (resolve(path) === resolve(seam)) continue;
    const source = readFileSync(path, 'utf8');
    if (/from\s+['"](?:@codemirror|@lezer)\//.test(source)) violations.push(relative(ROOT, path));
  }
  return violations;
}

export function analyzeCodeMirrorRuntime({ root = ROOT, uiDir = UI_DIR } = {}) {
  const uiPackage = readJson(join(uiDir, 'package.json'));
  const viteConfig = readFileSync(join(uiDir, 'vite.config.ts'), 'utf8');
  const errors = [];
  for (const packageName of CORE_PACKAGES) {
    const version = uiPackage.dependencies?.[packageName];
    if (!version || !/^\d+\.\d+\.\d+$/.test(version)) {
      errors.push(`${packageName} must be a direct exact-version dependency (got ${JSON.stringify(version)})`);
    }
    if (!viteConfig.includes(`'${packageName}'`)) {
      errors.push(`Vite resolve.dedupe is missing ${packageName}`);
    }
  }

  const closure = collectRuntimeClosure();
  for (const [packageName, dirs] of closure) {
    if (dirs.size > 1 && !viteConfig.includes(`'${packageName}'`)) {
      errors.push(`${packageName} resolves to multiple runtime instances without Vite dedupe: ${[...dirs].join(', ')}`);
    }
  }
  const businessImports = findBusinessImports();
  if (businessImports.length) {
    errors.push(`renderer code imports CodeMirror/Lezer directly outside the shared seam: ${businessImports.join(', ')}`);
  }
  return {
    errors,
    closure: Object.fromEntries([...closure].map(([name, dirs]) => [name, [...dirs]])),
    businessImports,
    root,
  };
}

if (import.meta.main) {
  const result = analyzeCodeMirrorRuntime();
  for (const [name, dirs] of Object.entries(result.closure)) {
    console.log(`[check:codemirror-runtime] ${name}: ${dirs.length} reachable instance(s)`);
  }
  if (result.errors.length) {
    for (const error of result.errors) console.error(`[check:codemirror-runtime] ERROR: ${error}`);
    process.exitCode = 1;
  } else {
    console.log('[check:codemirror-runtime] runtime closure and shared seam are valid');
  }
}
