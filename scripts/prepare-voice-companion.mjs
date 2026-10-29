#!/usr/bin/env bun
// Tauri beforeBuildCommand: compile the production daemon before collecting resources.
import { spawnSync } from 'node:child_process';
import { copyFile, mkdir, chmod, rm } from 'node:fs/promises';
import { existsSync, readdirSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
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

/**
 * Resolves official Microsoft Visual C++ Redistributable CRT files for app-local deployment.
 * Searches explicit environment overrides, vswhere discovery, and standard Visual Studio paths.
 * Validates that all required DLLs exist and match the target CPU architecture.
 */
export function resolveVcRedistCrt(arch = 'x64', env = process.env) {
  const normalizedArch = normalizeArch(arch);
  const msvcArch = normalizedArch === 'aarch64' ? 'arm64' : 'x64';

  const candidates = [];
  if (env.VC_REDIST_CRT_DIR) {
    candidates.push(env.VC_REDIST_CRT_DIR);
  } else {
    if (env.VCToolsRedistDir) {
      candidates.push(join(env.VCToolsRedistDir, msvcArch, 'Microsoft.VC143.CRT'));
      candidates.push(join(env.VCToolsRedistDir, msvcArch, 'Microsoft.VC142.CRT'));
    }

    if (process.platform === 'win32') {
      const vswhere = `${env['ProgramFiles(x86)'] ?? 'C:\\Program Files (x86)'}\\Microsoft Visual Studio\\Installer\\vswhere.exe`;
      if (existsSync(vswhere)) {
        try {
          const query = spawnSync(vswhere, ['-latest', '-products', '*', '-find', `VC\\Redist\\MSVC\\*\\${msvcArch}\\Microsoft.VC*.CRT`], {
            encoding: 'utf8',
            windowsHide: true,
          });
          if (!query.error && query.status === 0 && query.stdout) {
            for (const line of query.stdout.split(/\r?\n/)) {
              const trimmed = line.trim();
              if (trimmed && existsSync(trimmed)) candidates.push(trimmed);
            }
          }
        } catch {}
      }

      const programFiles = [env.ProgramFiles ?? 'C:\\Program Files', env['ProgramFiles(x86)'] ?? 'C:\\Program Files (x86)'];
      const editions = ['Enterprise', 'Community', 'Professional', 'BuildTools'];
      for (const pf of programFiles) {
        for (const edition of editions) {
          const msvcRoot = join(pf, 'Microsoft Visual Studio', '2022', edition, 'VC', 'Redist', 'MSVC');
          if (existsSync(msvcRoot)) {
            try {
              for (const ver of readdirSync(msvcRoot)) {
                candidates.push(join(msvcRoot, ver, msvcArch, 'Microsoft.VC143.CRT'));
                candidates.push(join(msvcRoot, ver, msvcArch, 'Microsoft.VC142.CRT'));
              }
            } catch {}
          }
        }
      }
    }
  }

  for (const dir of candidates) {
    if (!existsSync(dir)) continue;
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

  throw new Error(
    `Official Microsoft Visual C++ redistributable files not found for ${arch} (expected Microsoft.VC143.CRT containing ${REQUIRED_VC_REDIST_DLLS.join(', ')})`
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
