import { expect, test } from 'bun:test';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  voiceBuildPlan,
  stageVoiceCompanion,
  builtVoiceExecutable,
  resolveVcRedistCrt,
  REQUIRED_VC_REDIST_DLLS,
} from './prepare-voice-companion.mjs';

function pe(machine) {
  const buffer = Buffer.alloc(0x100);
  buffer[0] = 0x4d;
  buffer[1] = 0x5a;
  buffer.writeUInt32LE(0x80, 0x3c);
  buffer.write('PE\0\0', 0x80, 'ascii');
  buffer.writeUInt16LE(machine, 0x84);
  return buffer;
}

test('voice packaging uses the same target and debug intent as the desktop hook', () => {
  const debug = voiceBuildPlan({ TAURI_ENV_TARGET_TRIPLE: 'aarch64-pc-windows-msvc', TAURI_ENV_DEBUG: 'true' });
  expect(debug.triples).toEqual(['aarch64-pc-windows-msvc']);
  expect(debug.profile).toBe('debug');
  expect(debug.binary).toBe('voice-companion.exe');
  const release = voiceBuildPlan({ TAURI_ENV_TARGET_TRIPLE: 'x86_64-unknown-linux-gnu', TAURI_ENV_DEBUG: 'false' });
  expect(release.profile).toBe('release');
  expect(release.binary).toBe('voice-companion');
  expect(voiceBuildPlan({ TAURI_ENV_TARGET_TRIPLE: 'universal-apple-darwin' }).triples).toEqual([
    'aarch64-apple-darwin',
    'x86_64-apple-darwin',
  ]);
});

test('packaging takes the actual Cargo executable and refuses skeleton or missing artifacts', () => {
  const artifact = {
    reason: 'compiler-artifact',
    target: { name: 'voice-companion', kind: ['bin'] },
    features: [],
    executable: 'C:/isolated/voice-companion.exe',
  };
  expect(builtVoiceExecutable(JSON.stringify(artifact))).toBe(artifact.executable);
  expect(() => builtVoiceExecutable(JSON.stringify({ ...artifact, features: ['voice-skeleton'] }))).toThrow('skeleton');
  expect(() => builtVoiceExecutable(JSON.stringify({ ...artifact, executable: null }))).toThrow('executable');
});

test('voice resources include the actual executable and existing licenses; failure cannot reuse an old binary', async () => {
  const root = await mkdtemp(join(tmpdir(), 'flowy-voice-package-'));
  try {
    const source = join(root, 'build', 'voice-companion.exe');
    const out = join(root, 'bin');
    await mkdir(join(root, 'build'));
    await writeFile(source, 'test-only artifact');
    await mkdir(out);
    await writeFile(join(out, 'ai-mate.exe'), 'stale legacy daemon fixture');
    const license = join(root, 'LICENSE');
    await writeFile(license, 'test-only license fixture');
    await stageVoiceCompanion(source, out, 'voice-companion.exe', { LICENSE: license });
    expect(await readFile(join(out, 'voice-companion.exe'), 'utf8')).toBe('test-only artifact');
    await expect(readFile(join(out, 'ai-mate.exe'))).rejects.toThrow();
    expect(await readFile(join(out, 'voice-licenses', 'LICENSE'), 'utf8')).toBe('test-only license fixture');
    await expect(stageVoiceCompanion(join(root, 'missing'), out, 'voice-companion.exe', { LICENSE: license })).rejects.toThrow();
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

async function writeCrt(dir, machine) {
  await mkdir(dir, { recursive: true });
  for (const dll of REQUIRED_VC_REDIST_DLLS) {
    await writeFile(join(dir, dll), pe(machine));
  }
}

function isolatedProgramFiles(root) {
  return { ProgramFiles: join(root, 'pf'), 'ProgramFiles(x86)': join(root, 'pf86') };
}

test('VC++ runtime redistributable resolution finds and validates required CRT DLLs', async () => {
  const root = await mkdtemp(join(tmpdir(), 'flowy-vc-redist-fixture-'));
  try {
    const crtDir = join(root, 'Microsoft.VC143.CRT');
    await mkdir(crtDir, { recursive: true });
    for (const dll of REQUIRED_VC_REDIST_DLLS) {
      await writeFile(join(crtDir, dll), pe(0x8664));
    }
    const resolved = resolveVcRedistCrt('x64', { VC_REDIST_CRT_DIR: crtDir });
    for (const dll of REQUIRED_VC_REDIST_DLLS) {
      expect(resolved[dll]).toBe(join(crtDir, dll));
    }

    // Missing one DLL fails
    await rm(join(crtDir, 'msvcp140.dll'));
    expect(() => resolveVcRedistCrt('x64', { VC_REDIST_CRT_DIR: crtDir })).toThrow('not found');

    // Wrong architecture fails
    await writeFile(join(crtDir, 'msvcp140.dll'), pe(0xaa64)); // arm64 DLL for x64 request
    expect(() => resolveVcRedistCrt('x64', { VC_REDIST_CRT_DIR: crtDir })).toThrow('not found');
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('VC++ runtime resolution finds the VS 2026 ARM64 CRT and prefers it over VC143', async () => {
  const root = await mkdtemp(join(tmpdir(), 'flowy-vc-redist-vs18-'));
  try {
    const env = isolatedProgramFiles(root);
    const latest = join(
      env.ProgramFiles,
      'Microsoft Visual Studio',
      '18',
      'Enterprise',
      'VC',
      'Redist',
      'MSVC',
      '14.50.35717',
      'arm64',
      'Microsoft.VC145.CRT'
    );
    const previous = join(
      env.ProgramFiles,
      'Microsoft Visual Studio',
      '2022',
      'Enterprise',
      'VC',
      'Redist',
      'MSVC',
      '14.44.35207',
      'arm64',
      'Microsoft.VC143.CRT'
    );
    const alias = join(
      env.ProgramFiles,
      'Microsoft Visual Studio',
      '18',
      'Enterprise',
      'VC',
      'Redist',
      'MSVC',
      'v145',
      'arm64',
      'Microsoft.VC145.CRT'
    );
    const debug = join(
      env.ProgramFiles,
      'Microsoft Visual Studio',
      '18',
      'Enterprise',
      'VC',
      'Redist',
      'MSVC',
      '14.50.35717',
      'arm64',
      'Microsoft.VC145.DebugCRT'
    );
    await writeCrt(latest, 0xaa64);
    await writeCrt(previous, 0xaa64);
    await writeCrt(alias, 0xaa64);
    await writeCrt(debug, 0xaa64);

    const resolved = resolveVcRedistCrt('arm64', env);
    expect(resolved['vcruntime140.dll']).toBe(join(latest, 'vcruntime140.dll'));

    await rm(latest, { recursive: true, force: true });
    await rm(alias, { recursive: true, force: true });
    const fallback = resolveVcRedistCrt('arm64', env);
    expect(fallback['msvcp140.dll']).toBe(join(previous, 'msvcp140.dll'));

    await rm(previous, { recursive: true, force: true });
    expect(() => resolveVcRedistCrt('arm64', env)).toThrow('not found');
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('VC++ runtime resolution keeps a complete older CRT when the newer directory is the wrong architecture', async () => {
  const root = await mkdtemp(join(tmpdir(), 'flowy-vc-redist-arch-'));
  try {
    const env = isolatedProgramFiles(root);
    const newer = join(env.ProgramFiles, 'Microsoft Visual Studio', '18', 'Enterprise', 'VC', 'Redist', 'MSVC', '14.50.35717', 'arm64', 'Microsoft.VC145.CRT');
    const older = join(env.ProgramFiles, 'Microsoft Visual Studio', '2022', 'BuildTools', 'VC', 'Redist', 'MSVC', '14.40.33807', 'arm64', 'Microsoft.VC143.CRT');
    await writeCrt(newer, 0x8664);
    await writeCrt(older, 0xaa64);
    const resolved = resolveVcRedistCrt('aarch64', env);
    expect(resolved['vcruntime140.dll']).toBe(join(older, 'vcruntime140.dll'));
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('VC++ runtime resolution reads VCToolsRedistDir toolset folders other than VC143', async () => {
  const root = await mkdtemp(join(tmpdir(), 'flowy-vc-redist-tools-'));
  try {
    const crtDir = join(root, 'MSVC', '14.50.35717', 'x64', 'Microsoft.VC145.CRT');
    await writeCrt(crtDir, 0x8664);
    const resolved = resolveVcRedistCrt('x64', {
      VCToolsRedistDir: join(root, 'MSVC', '14.50.35717'),
      ...isolatedProgramFiles(root),
    });
    expect(resolved['msvcp140_1.dll']).toBe(join(crtDir, 'msvcp140_1.dll'));
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('stageVoiceCompanion deploys VC++ runtime DLLs app-locally into bin/', async () => {
  const root = await mkdtemp(join(tmpdir(), 'flowy-stage-vc-'));
  try {
    const source = join(root, 'voice-companion.exe');
    await writeFile(source, 'test-only daemon');
    const out = join(root, 'bin');
    await mkdir(out);
    const crtDir = join(root, 'crt');
    await mkdir(crtDir);
    const vcRedist = {};
    for (const dll of REQUIRED_VC_REDIST_DLLS) {
      const p = join(crtDir, dll);
      await writeFile(p, `content of ${dll}`);
      vcRedist[dll] = p;
    }
    await stageVoiceCompanion(source, out, 'voice-companion.exe', {}, vcRedist);
    for (const dll of REQUIRED_VC_REDIST_DLLS) {
      expect(await readFile(join(out, dll), 'utf8')).toBe(`content of ${dll}`);
    }
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
