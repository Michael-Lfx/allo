import { expect, test } from 'bun:test';
import { mkdtemp, mkdir, rm, symlink, writeFile, chmod } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import {
  REQUIRED_LICENSES,
  REQUIRED_VC_REDIST_DLLS,
  findSquashfsOffset,
  verifyExtractedTree,
} from './verify-voice-release-package.mjs';

const SCRIPT = join(import.meta.dir, 'verify-voice-release-package.mjs');

function pe(machine) {
  const buffer = Buffer.alloc(0x100);
  buffer[0] = 0x4d;
  buffer[1] = 0x5a;
  buffer.writeUInt32LE(0x80, 0x3c);
  buffer.write('PE\0\0', 0x80, 'ascii');
  buffer.writeUInt16LE(machine, 0x84);
  return buffer;
}

function elf(machine) {
  const buffer = Buffer.alloc(0x40);
  buffer[0] = 0x7f;
  buffer.write('ELF', 1, 'ascii');
  buffer[4] = 2;
  buffer[5] = 1;
  buffer.writeUInt16LE(machine, 0x12);
  return buffer;
}

function macho(cputype) {
  const buffer = Buffer.alloc(32);
  buffer.writeUInt32BE(0xfeedfacf, 0);
  buffer.writeUInt32BE(cputype, 4);
  return buffer;
}

function fatMacho(cputypes) {
  const buffer = Buffer.alloc(8 + cputypes.length * 20);
  buffer.writeUInt32BE(0xcafebabe, 0);
  buffer.writeUInt32BE(cputypes.length, 4);
  cputypes.forEach((cpu, index) => buffer.writeUInt32BE(cpu, 8 + index * 20));
  return buffer;
}

function makeSquashfsSuperblock(blockSize = 131072, major = 4, minor = 0) {
  const buf = Buffer.alloc(96);
  buf.writeUInt32LE(0x73717368, 0); // magic
  buf.writeUInt32LE(1, 4); // inodes
  buf.writeUInt32LE(0, 8); // mtime
  buf.writeUInt32LE(blockSize, 12); // block_size
  buf.writeUInt32LE(0, 16); // fragments
  buf.writeUInt16LE(1, 20); // compression
  buf.writeUInt16LE(17, 22); // block_log
  buf.writeUInt16LE(0, 24); // flags
  buf.writeUInt16LE(1, 26); // id_count
  buf.writeUInt16LE(major, 28); // version_major
  buf.writeUInt16LE(minor, 30); // version_minor
  return buf;
}

async function makeTree({
  platform = 'windows',
  daemon = true,
  archBytes,
  licenses = REQUIRED_LICENSES,
  rg = true,
  legacy = false,
  arch = 'x64',
  vcRedist = true,
  vcArchBytes,
} = {}) {
  const root = await mkdtemp(join(tmpdir(), 'flowy-voice-fixture-'));
  const bin = join(root, 'bin');
  await mkdir(bin, { recursive: true });
  const exe = platform === 'windows' ? 'voice-companion.exe' : 'voice-companion';
  const rgName = platform === 'windows' ? 'rg.exe' : 'rg';
  const bytes = archBytes ?? (platform === 'windows' ? pe(0x8664) : elf(0x3e));
  if (platform === 'windows') {
    await writeFile(join(root, 'Flowy.exe'), 'test-only host location');
  }
  if (daemon) await writeFile(join(bin, exe), bytes);
  if (rg) await writeFile(join(bin, rgName), bytes);
  if (legacy) await writeFile(join(bin, platform === 'windows' ? 'ai-mate.exe' : 'ai-mate'), pe(0x8664));
  if (licenses.length) {
    await mkdir(join(bin, 'voice-licenses'), { recursive: true });
    for (const name of licenses) await writeFile(join(bin, 'voice-licenses', name), `${name}\n`);
  }
  if (platform === 'windows' && vcRedist) {
    const vcBytes = vcArchBytes ?? pe(0x8664);
    for (const dll of REQUIRED_VC_REDIST_DLLS) {
      await writeFile(join(bin, dll), vcBytes);
    }
  }
  return { root, arch };
}

test('a complete Windows resource tree passes with daemon, architecture, licenses, rg, and vc_redist', async () => {
  const { root, arch } = await makeTree();
  try {
    const { problems } = verifyExtractedTree(root, { platform: 'windows', arch });
    expect(problems).toEqual([]);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('missing daemon fails', async () => {
  const { root, arch } = await makeTree({ daemon: false });
  try {
    expect(verifyExtractedTree(root, { platform: 'windows', arch }).problems.some((p) => p.includes('missing voice-companion.exe'))).toBe(true);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('wrong daemon architecture fails', async () => {
  const { root, arch } = await makeTree({ archBytes: pe(0xaa64) });
  try {
    expect(verifyExtractedTree(root, { platform: 'windows', arch }).problems.some((p) => p.includes('does not match expected x86_64'))).toBe(true);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('a missing required voice license fails', async () => {
  const { root, arch } = await makeTree({ licenses: ['Flowy-LICENSE'] });
  try {
    expect(verifyExtractedTree(root, { platform: 'windows', arch }).problems.some((p) => p.includes('sherpa-onnx-LICENSE'))).toBe(true);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('the existing bundled rg must stay in the package', async () => {
  const { root, arch } = await makeTree({ rg: false });
  try {
    expect(verifyExtractedTree(root, { platform: 'windows', arch }).problems.some((p) => p.includes('rg.exe'))).toBe(true);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('a stale legacy ai-mate daemon must not ride along', async () => {
  const { root, arch } = await makeTree({ legacy: true });
  try {
    expect(verifyExtractedTree(root, { platform: 'windows', arch }).problems.some((p) => p.includes('legacy ai-mate.exe'))).toBe(true);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('missing VC++ runtime DLL fails on Windows', async () => {
  const { root, arch } = await makeTree({ vcRedist: false });
  try {
    const { problems } = verifyExtractedTree(root, { platform: 'windows', arch });
    expect(problems.some((p) => p.includes('required VC++ runtime DLL'))).toBe(true);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('wrong architecture VC++ runtime DLL fails on Windows', async () => {
  const { root, arch } = await makeTree({ vcArchBytes: pe(0xaa64) });
  try {
    const { problems } = verifyExtractedTree(root, { platform: 'windows', arch });
    expect(problems.some((p) => p.includes('VC++ runtime DLL') && p.includes('does not match expected x86_64'))).toBe(true);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('daemon placed only in unrelated/bin is rejected (misplaced daemon)', async () => {
  const root = await mkdtemp(join(tmpdir(), 'flowy-misplaced-'));
  try {
    await writeFile(join(root, 'Flowy.exe'), 'test-only host');
    const unrelatedBin = join(root, 'unrelated', 'bin');
    await mkdir(unrelatedBin, { recursive: true });
    await writeFile(join(unrelatedBin, 'voice-companion.exe'), pe(0x8664));
    await writeFile(join(unrelatedBin, 'rg.exe'), pe(0x8664));
    const { problems } = verifyExtractedTree(root, { platform: 'windows', arch: 'x64' });
    expect(problems.some((p) => p.includes('missing voice-companion.exe'))).toBe(true);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('out-of-package symlink is ignored and fails if package lacks daemon', async () => {
  const root = await mkdtemp(join(tmpdir(), 'flowy-escape-'));
  try {
    const outside = join(root, 'outside');
    await mkdir(join(outside, 'bin'), { recursive: true });
    await writeFile(join(outside, 'bin', 'voice-companion.exe'), pe(0x8664));
    const pkg = join(root, 'pkg');
    await mkdir(pkg);
    await writeFile(join(pkg, 'Flowy.exe'), 'test-only host');
    await symlink(resolve(outside), join(pkg, 'Applications'), 'junction');
    const { problems } = verifyExtractedTree(pkg, { platform: 'windows', arch: 'x64' });
    expect(problems.some((p) => p.includes('missing voice-companion.exe'))).toBe(true);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('licenses directory escaping via external junction is rejected', async () => {
  const root = await mkdtemp(join(tmpdir(), 'flowy-lic-escape-'));
  try {
    const pkg = join(root, 'pkg');
    const bin = join(pkg, 'bin');
    const outside = join(root, 'outside');
    await mkdir(bin, { recursive: true });
    await mkdir(join(outside, 'licenses'), { recursive: true });
    await writeFile(join(pkg, 'Flowy.exe'), 'test-only host');
    await writeFile(join(bin, 'voice-companion.exe'), pe(0x8664));
    await writeFile(join(bin, 'rg.exe'), pe(0x8664));
    for (const dll of REQUIRED_VC_REDIST_DLLS) await writeFile(join(bin, dll), pe(0x8664));
    for (const name of REQUIRED_LICENSES) await writeFile(join(outside, 'licenses', name), 'outside license');

    // Create junction pointing to external directory
    await symlink(resolve(join(outside, 'licenses')), join(bin, 'voice-licenses'), 'junction');
    const { problems } = verifyExtractedTree(pkg, { platform: 'windows', arch: 'x64' });
    expect(problems.some((p) => p.includes('escapes package root via link'))).toBe(true);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('legal package-internal junction is accepted', async () => {
  const root = await mkdtemp(join(tmpdir(), 'flowy-internal-link-'));
  try {
    const pkg = join(root, 'pkg');
    const bin = join(pkg, 'bin');
    const internalLic = join(pkg, 'internal-licenses');
    await mkdir(bin, { recursive: true });
    await mkdir(internalLic, { recursive: true });
    await writeFile(join(pkg, 'Flowy.exe'), 'test-only host');
    await writeFile(join(bin, 'voice-companion.exe'), pe(0x8664));
    await writeFile(join(bin, 'rg.exe'), pe(0x8664));
    for (const dll of REQUIRED_VC_REDIST_DLLS) await writeFile(join(bin, dll), pe(0x8664));
    for (const name of REQUIRED_LICENSES) await writeFile(join(internalLic, name), 'internal license');

    // Link inside package
    await symlink(resolve(internalLic), join(bin, 'voice-licenses'), 'junction');
    const { problems } = verifyExtractedTree(pkg, { platform: 'windows', arch: 'x64' });
    expect(problems).toEqual([]);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('a universal macOS app requires both slices in the daemon', async () => {
  const root = await mkdtemp(join(tmpdir(), 'flowy-voice-fixture-'));
  try {
    const bin = join(root, 'Flowy.app/Contents/Resources/bin');
    await mkdir(join(bin, 'voice-licenses'), { recursive: true });
    await writeFile(join(bin, 'voice-companion'), fatMacho([0x0100000c, 0x01000007]));
    await writeFile(join(bin, 'rg'), fatMacho([0x0100000c, 0x01000007]));
    for (const name of REQUIRED_LICENSES) await writeFile(join(bin, 'voice-licenses', name), `${name}\n`);
    expect(verifyExtractedTree(root, { platform: 'macos', arch: 'universal' }).problems).toEqual([]);
    expect(verifyExtractedTree(root, { platform: 'macos', arch: 'x86_64' }).problems).toEqual([]);
    await writeFile(join(bin, 'voice-companion'), macho(0x01000007));
    expect(verifyExtractedTree(root, { platform: 'macos', arch: 'universal' }).problems.some((p) => p.includes('does not match expected universal'))).toBe(true);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('findSquashfsOffset detects raw squashfs and AppImage with ELF prefix', async () => {
  const root = await mkdtemp(join(tmpdir(), 'flowy-squashfs-'));
  try {
    // 1. Raw squashfs at offset 0
    const rawPath = join(root, 'raw.sqfs');
    await writeFile(rawPath, makeSquashfsSuperblock());
    expect(findSquashfsOffset(rawPath)).toBe(0);

    // 2. AppImage with 1024 bytes ELF prefix
    const appImagePath = join(root, 'app.AppImage');
    const elfHeader = Buffer.alloc(1024);
    elfHeader[0] = 0x7f;
    elfHeader.write('ELF', 1, 'ascii');
    await writeFile(appImagePath, Buffer.concat([elfHeader, makeSquashfsSuperblock()]));
    expect(findSquashfsOffset(appImagePath)).toBe(1024);

    // 3. Corrupted superblock (version_major != 4)
    const badPath = join(root, 'bad.AppImage');
    await writeFile(badPath, Buffer.concat([elfHeader, makeSquashfsSuperblock(131072, 3, 0)]));
    expect(findSquashfsOffset(badPath)).toBeNull();

    // 4. Truncated file
    const truncatedPath = join(root, 'truncated.AppImage');
    await writeFile(truncatedPath, Buffer.from('too short'));
    expect(findSquashfsOffset(truncatedPath)).toBeNull();
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('live Linux SquashFS AppImage unpacking integration test (when tools available)', async () => {
  const probeMk = spawnSync('mksquashfs', ['-version'], { stdio: 'ignore' });
  const probeUn = spawnSync('unsquashfs', ['-v'], { stdio: 'ignore' });
  if (probeMk.error || probeUn.error || probeMk.status !== 0) {
    // Host lacks native squashfs tools (e.g. Windows).
    console.log('[R3 test note] mksquashfs/unsquashfs not installed on host platform; live integration test runs in Linux CI.');
    return;
  }

  const root = await mkdtemp(join(tmpdir(), 'flowy-live-appimage-'));
  try {
    const staging = join(root, 'staging');
    const bin = join(staging, 'usr', 'lib', 'Flowy', 'bin');
    await mkdir(bin, { recursive: true });
    await writeFile(join(bin, 'voice-companion'), elf(0x3e));
    await writeFile(join(bin, 'rg'), elf(0x3e));
    await mkdir(join(bin, 'voice-licenses'), { recursive: true });
    for (const name of REQUIRED_LICENSES) await writeFile(join(bin, 'voice-licenses', name), 'valid license\n');

    const sqfs = join(root, 'fs.sqfs');
    const mkResult = spawnSync('mksquashfs', [staging, sqfs, '-noappend', '-all-root'], { encoding: 'utf8' });
    expect(mkResult.status).toBe(0);

    // Build AppImage Type 2 fixture by prepending 2048 bytes ELF runtime header
    const appimage = join(root, 'Flowy_1.6.9_x64.AppImage');
    const elfHeader = Buffer.alloc(2048);
    elfHeader[0] = 0x7f;
    elfHeader.write('ELF', 1, 'ascii');
    const sqfsBytes = await Bun.file(sqfs).arrayBuffer();
    await writeFile(appimage, Buffer.concat([elfHeader, Buffer.from(sqfsBytes)]));
    await chmod(appimage, 0o755);

    // Run official CLI verification
    const cliResult = runCli(['--platform', 'linux', '--arch', 'x64', '--artifact', appimage]);
    expect(cliResult.status).toBe(0);
    expect(cliResult.stdout).toContain('VERIFIED');
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

function runCli(args) {
  return spawnSync(process.execPath, [SCRIPT, ...args], { encoding: 'utf8' });
}

test('the CLI exits zero for a complete package tree and nonzero for each missing component', async () => {
  const good = await makeTree();
  const missingDaemon = await makeTree({ daemon: false });
  const wrongArch = await makeTree({ archBytes: pe(0xaa64) });
  const missingLicense = await makeTree({ licenses: ['Flowy-LICENSE'] });
  const missingVc = await makeTree({ vcRedist: false });
  try {
    const ok = runCli(['--platform', 'windows', '--arch', 'x64', '--artifact', good.root]);
    expect(ok.status).toBe(0);
    expect(ok.stdout).toContain('VERIFIED');

    for (const [fixture, fragment] of [
      [missingDaemon, 'missing voice-companion.exe'],
      [wrongArch, 'does not match expected x86_64'],
      [missingLicense, 'sherpa-onnx-LICENSE'],
      [missingVc, 'required VC++ runtime DLL'],
    ]) {
      const result = runCli(['--platform', 'windows', '--arch', 'x64', '--artifact', fixture.root]);
      expect(result.status).not.toBe(0);
      expect(result.stderr).toContain(fragment);
    }
  } finally {
    for (const fixture of [good, missingDaemon, wrongArch, missingLicense, missingVc]) {
      await rm(fixture.root, { recursive: true, force: true });
    }
  }
});
