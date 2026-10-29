import { existsSync, readFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';

export function viteOptimizeDepsDir(uiRoot) {
  return join(uiRoot, 'node_modules', '.vite', 'deps');
}

/**
 * Collect the on-disk filenames Vite's dep optimizer claims to have written.
 * Both `optimized` (named entries) and `chunks` (shared helpers) must exist or
 * the Tauri webview 404s mid-import and paints a white screen.
 */
export function listedViteOptimizeDepFiles(metadata) {
  const files = [];
  for (const group of [metadata?.optimized, metadata?.chunks]) {
    if (!group || typeof group !== 'object' || Array.isArray(group)) continue;
    for (const entry of Object.values(group)) {
      if (entry && typeof entry === 'object' && typeof entry.file === 'string' && entry.file.length > 0) {
        files.push(entry.file);
      }
    }
  }
  return files;
}

/**
 * Vite's dep metadata encodes `.` in some chunk filenames as `__`
 * (`subset-worker.chunk-XXXX.js` → `subset-worker__chunk-XXXX.js`). The
 * file on disk keeps the original dots. Treat either name as present.
 */
export function viteOptimizeDepDiskNames(file) {
  if (typeof file !== 'string' || file.length === 0) return [];
  const decoded = file.replaceAll('__', '.');
  return decoded === file ? [file] : [file, decoded];
}

function optimizeDepFileExists(depsDir, file, exists) {
  return viteOptimizeDepDiskNames(file).some((name) => exists(join(depsDir, name)));
}

export function inspectViteOptimizeDeps(
  depsDir,
  { exists = existsSync, readFile = readFileSync } = {},
) {
  const metadataPath = join(depsDir, '_metadata.json');
  if (!exists(metadataPath)) {
    return { status: 'missing' };
  }
  let metadata;
  try {
    metadata = JSON.parse(readFile(metadataPath, 'utf8'));
  } catch (error) {
    return { status: 'invalid', error: String(error?.message ?? error) };
  }
  const files = listedViteOptimizeDepFiles(metadata);
  const missing = files.filter((file) => !optimizeDepFileExists(depsDir, file, exists));
  if (missing.length > 0) {
    return { status: 'incomplete', missing, files };
  }
  return { status: 'intact', files };
}

/**
 * A partial re-optimize can delete shared chunks while `_metadata.json` still
 * names them (or while the webview still imports the previous names). Wipe the
 * cache so the next Vite boot rebuilds a consistent graph. A missing cache is
 * a cold start, not an error.
 */
export function reconcileViteOptimizeDeps(
  uiRoot,
  { exists = existsSync, readFile = readFileSync, remove = rmSync, log = console.log } = {},
) {
  const depsDir = viteOptimizeDepsDir(uiRoot);
  const inspection = inspectViteOptimizeDeps(depsDir, { exists, readFile });
  if (inspection.status === 'incomplete' || inspection.status === 'invalid') {
    log(`[run-dev] resetting Vite optimize deps cache (${inspection.status})`);
    remove(depsDir, { recursive: true, force: true });
    return { action: 'reset', depsDir, ...inspection };
  }
  return {
    action: inspection.status === 'missing' ? 'cold' : 'intact',
    depsDir,
    ...inspection,
  };
}

export function viteOptimizeDepsIsIntact(
  uiRoot,
  { exists = existsSync, readFile = readFileSync } = {},
) {
  return inspectViteOptimizeDeps(viteOptimizeDepsDir(uiRoot), { exists, readFile }).status === 'intact';
}
