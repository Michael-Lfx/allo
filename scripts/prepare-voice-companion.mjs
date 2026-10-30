#!/usr/bin/env bun
// Tauri beforeBuildCommand: compile the production daemon before collecting resources.
import { spawnSync } from 'node:child_process';
import { copyFile, mkdir, chmod, rm } from 'node:fs/promises';
import { existsSync, readdirSync, statSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { resolveRgTarget } from './ensure-bundled-rg.mjs';
import { binaryArchs, normalizeArch, REQUIRED_VC_REDIST_DLLS } from './verify-voice-release-package.mjs';

export { REQUIRED_VC_REDIST_DLLS };

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

export function voiceBuildPlan(env = process.env) {
  const target = resolveRgTarget(env);
  return {
    triples: target.arch === 'universal' ? ['aarch64-apple-darwin', 'x86_64-apple-darwin'] : [target.triple],
    profile: env.TAURI_ENV_DEBUG === 'true' ? 'debug' : 'release',
    binary: target.platform === 'win32' ? 'voice-companion.exe' : 'voice-companion',
  };
}

export function builtVoiceExecutable(output) {
  let executable;
  for (const line of output.split('\n')) {
    let item;
    try {
      item = JSON.parse(line);
    } catch {
      continue;
    }
    if (item.reason !== 'compiler-artifact' || item.target?.name !== 'voice-companion' || !item.target.kind.includes('bin')) continue;
    if (item.features?.includes('voice-skeleton')) throw new Error('Refusing to package a voice-skeleton executable');
    if (item.executable) executable = item.executable;
  }
  if (!executable) throw new Error('Cargo did not produce the voice-companion executable');
  return executable;
}

/** Release CRT directories only. Excludes Microsoft.VC145.DebugCRT and similar. */
const VC_CRT_DIR_RE = /^Microsoft\.VC\d+\.CRT$/i;

function listDirNames(dir) {
  try {
    return readdirSync(dir, { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .map((entry) => entry.name);
  } catch {
    return [];
  }
}

function crtDirsInArch(archDir) {
  const named = listDirNames(archDir)
    .filter((name) => VC_CRT_DIR_RE.test(name))
    .map((name) => join(archDir, name));
  if (named.length) return named;
  // A few installs drop the four CRT DLLs directly in the arch folder.
  if (REQUIRED_VC_REDIST_DLLS.every((dll) => existsSync(join(archDir, dll)))) return [archDir];
  return [];
}

function crtDirsInRedist(msvcRoot, msvcArch) {
  const found = [];
  for (const ver of listDirNames(msvcRoot)) {
    found.push(...crtDirsInArch(join(msvcRoot, ver, msvcArch)));
  }
  return found;
}

/**
 * Roots to scan. An explicit ProgramFiles key, even when its sibling is absent,
 * disables the machine-wide default so tests cannot pick up a developer install.
 */
function programFileRoots(env) {
  const keys = ['ProgramFiles', 'ProgramFiles(x86)'];
  if (keys.some((key) => Object.prototype.hasOwnProperty.call(env, key))) {
    return [...new Set(keys.filter((key) => env[key]).map((key) => env[key]))];
  }
  if (process.platform !== 'win32') return [];
  return ['C:\\Program Files', 'C:\\Program Files (x86)'];
}

function queryVswhere(vswhere, msvcArch) {
  try {
    const query = spawnSync(vswhere, ['-latest', '-products', '*', '-find', `VC\\Redist\\MSVC\\*\\${msvcArch}\\Microsoft.VC*.CRT`], {
      encoding: 'utf8',
      windowsHide: true,
    });
    if (query.error || query.status !== 0 || !query.stdout) return [];
    return query.stdout
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter((line) => line && VC_CRT_DIR_RE.test(basename(line)) && existsSync(line));
  } catch {
    return [];
  }
}

/**
 * Newer redist first. A v14 CRT is backward compatible, and the copy we ship
 * must be at least as new as the toolset that linked the binary. Folder names
 * like `v143` are toolset aliases, not version numbers above `14.50`.
 */
function compareCrtDesc(a, b) {
  const rank = (dir) => {
    const token = dir.match(/[\\/]MSVC[\\/]([^\\/]+)[\\/]/i)?.[1] ?? '';
    const version = /^v\d+$/i.test(token) ? [] : (token.match(/\d+/g) ?? []).map((part) => Number.parseInt(part, 10));
    const generation = Number.parseInt(dir.match(/Microsoft\.VC(\d+)\.CRT/i)?.[1] ?? '0', 10);
    return { version, generation };
  };
  const left = rank(a);
  const right = rank(b);
  if (right.generation !== left.generation) return right.generation - left.generation;
  const length = Math.max(left.version.length, right.version.length);
  for (let i = 0; i < length; i += 1) {
    const diff = (right.version[i] ?? 0) - (left.version[i] ?? 0);
    if (diff) return diff;
  }
  return 0;
}

function collectVcRedistCandidates(msvcArch, env) {
  const candidates = [];
  if (env.VCToolsRedistDir) {
    const direct = crtDirsInArch(join(env.VCToolsRedistDir, msvcArch));
    candidates.push(...(direct.length ? direct : crtDirsInRedist(env.VCToolsRedistDir, msvcArch)));
  }
  if (env.VCINSTALLDIR) {
    candidates.push(...crtDirsInRedist(join(env.VCINSTALLDIR, 'Redist', 'MSVC'), msvcArch));
  }
  for (const root of programFileRoots(env)) {
    const vswhere = join(root, 'Microsoft Visual Studio', 'Installer', 'vswhere.exe');
    if (existsSync(vswhere)) candidates.push(...queryVswhere(vswhere, msvcArch));
    const vsRoot = join(root, 'Microsoft Visual Studio');
    for (const product of listDirNames(vsRoot)) {
      if (product.toLowerCase() === 'installer') continue;
      for (const edition of listDirNames(join(vsRoot, product))) {
        candidates.push(...crtDirsInRedist(join(vsRoot, product, edition, 'VC', 'Redist', 'MSVC'), msvcArch));
      }
    }
  }
  return [...new Set(candidates)].sort(compareCrtDesc);
}

/**
 * Resolves official Microsoft Visual C++ Redistributable CRT files for app-local deployment.
 * Searches VC_REDIST_CRT_DIR, VCToolsRedistDir, vswhere, and every installed Visual Studio
 * product (2022 VC143 and 2026 VC145 under Microsoft Visual Studio\18). The newest complete
 * CRT whose DLLs match the target CPU architecture wins.
 */
export function resolveVcRedistCrt(arch = 'x64', env = process.env) {
  const normalizedArch = normalizeArch(arch);
  const msvcArch = normalizedArch === 'aarch64' ? 'arm64' : 'x64';
  const candidates = env.VC_REDIST_CRT_DIR ? [env.VC_REDIST_CRT_DIR] : collectVcRedistCandidates(msvcArch, env);

  let inspected = 0;
  for (const dir of candidates) {
    if (!existsSync(dir)) continue;
    inspected += 1;
    const resolved = {};
    let ok = true;
    for (const dll of REQUIRED_VC_REDIST_DLLS) {
      const full = join(dir, dll);
      if (!existsSync(full) || statSync(full).size === 0) {
        ok = false;
        break;
      }
      try {
        const archs = binaryArchs(full);
        if (!archs.includes(normalizedArch)) {
          ok = false;
          break;
        }
      } catch {
        ok = false;
        break;
      }
      resolved[dll] = full;
    }
    if (ok) return resolved;
  }

  const checked = inspected
    ? `Checked ${inspected} CRT director${inspected === 1 ? 'y' : 'ies'}; none had a complete ${msvcArch} set.`
    : `No CRT directory was found. Install the Visual Studio ${msvcArch} C++ redistributable, or set VC_REDIST_CRT_DIR.`;
  throw new Error(
    `Official Microsoft Visual C++ redistributable files not found for ${arch} (expected a Microsoft.VC*.CRT directory containing ${REQUIRED_VC_REDIST_DLLS.join(', ')}). ${checked}`
  );
}

export async function stageVoiceCompanion(executable, outDir, binary, licenses, vcRedist = {}) {
  await mkdir(join(outDir, 'voice-licenses'), { recursive: true });
  // Never let a stale legacy daemon from a previous local build ride along:
  // it would mask a missing production binary and its feature set is unknown.
  for (const legacy of ['ai-mate.exe', 'ai-mate']) await rm(join(outDir, legacy), { force: true });
  await copyFile(executable, join(outDir, binary));
  if (!binary.endsWith('.exe')) await chmod(join(outDir, binary), 0o755);
  for (const [name, path] of Object.entries(licenses)) await copyFile(path, join(outDir, 'voice-licenses', name));
  for (const [name, path] of Object.entries(vcRedist)) await copyFile(path, join(outDir, name));
}

async function main() {
  const plan = voiceBuildPlan();
  const executables = [];
  for (const triple of plan.triples) {
    console.log(`[voice-package] building ${triple} ${plan.profile}`);
    const result = spawnSync(
      'cargo',
      [
        'build',
        '-p',
        'voice-companion',
        '--bin',
        'voice-companion',
        '--no-default-features',
        '--target',
        triple,
        '--message-format=json-render-diagnostics',
        ...(plan.profile === 'release' ? ['--release'] : []),
      ],
      { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'], windowsHide: true, maxBuffer: 64 * 1024 * 1024 }
    );
    if (result.error) throw result.error;
    if (result.status !== 0) {
      process.stderr.write(result.stdout || '');
      throw new Error(`voice-companion build failed for ${triple}: ${result.status}`);
    }
    executables.push(builtVoiceExecutable(result.stdout));
  }
  const outDir = join(root, 'apps', 'desktop', 'resources', 'bin');
  let executable = executables[0];
  if (executables.length > 1) {
    executable = join(dirname(executables[0]), 'voice-companion-universal');
    const result = spawnSync('lipo', ['-create', ...executables, '-output', executable], { stdio: 'inherit', windowsHide: true });
    if (result.error) throw result.error;
    if (result.status !== 0) throw new Error('lipo could not assemble the universal voice daemon');
  }

  let vcRedist = {};
  if (plan.binary.endsWith('.exe')) {
    const target = resolveRgTarget();
    vcRedist = resolveVcRedistCrt(target.arch, process.env);
    const sample = Object.values(vcRedist)[0];
    if (sample) console.log(`[voice-package] VC++ runtime ${dirname(sample)}`);
  }

  await stageVoiceCompanion(
    executable,
    outDir,
    plan.binary,
    {
      'Flowy-LICENSE': join(root, 'LICENSE'),
      'sherpa-onnx-LICENSE': join(root, 'crates', 'vendor', 'sherpa-onnx', 'LICENSE'),
    },
    vcRedist
  );
  console.log(`[voice-package] staged ${join(outDir, plan.binary)} with runtime dependencies`);
}

if (import.meta.main)
  main().catch((error) => {
    console.error(error);
    process.exit(1);
  });
