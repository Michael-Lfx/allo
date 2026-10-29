#!/usr/bin/env bun
/**
 * Final-installer gate for the voice companion.
 *
 * A staged `apps/desktop/resources/bin` proves nothing about the shipped
 * installer: the v1.6.9 release staged/prepared less than it packaged. This
 * gate opens the collected artifact and checks the daemon, its CPU
 * architecture, the voice licenses, the bundled ripgrep, and required runtime
 * dependencies (such as MSVC CRT on Windows).
 *
 *   bun run verify:voice-package -- --platform windows --arch x64 --dist dist/desktop
 *   bun run verify:voice-package -- --platform macos --arch arm64 --dist dist/desktop
 *   bun run verify:voice-package -- --platform macos --arch universal \
 *     --artifact target/universal-apple-darwin/release/bundle/macos/Flowy.app
 *   bun run verify:voice-package -- --platform linux --arch x64 --dist dist/desktop
 *
 * A directory argument is treated as an already-extracted package tree (used by
 * fixtures and debugging); real release steps pass installers/packages.
 * Exit codes: 0 verified, 1 missing/mismatched content, 2 unverified (tool or
 * platform unavailable). Exit 2 is deliberately nonzero: CI must never treat
 * "could not check" as a pass.
 */
import { createHash } from 'node:crypto';
import {
  closeSync,
  existsSync,
  lstatSync,
  openSync,
  readFileSync,
  readSync,
  readdirSync,
  realpathSync,
  statSync,
} from 'node:fs';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { basename, dirname, extname, isAbsolute, join, relative, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

export const REQUIRED_LICENSES = ['Flowy-LICENSE', 'sherpa-onnx-LICENSE'];
export const REQUIRED_VC_REDIST_DLLS = [
  'msvcp140.dll',
  'msvcp140_1.dll',
  'vcruntime140.dll',
  'vcruntime140_1.dll',
];

export class UnverifiedError extends Error {}

export function normalizeArch(value) {
  const arch = String(value ?? '').toLowerCase();
  if (arch === 'x64' || arch === 'x86_64' || arch === 'amd64') return 'x86_64';
  if (arch === 'arm64' || arch === 'aarch64') return 'aarch64';
  if (arch === 'universal' || arch === 'all') return 'universal';
  throw new Error(`unsupported --arch '${value}' (expected x64/x86_64, arm64/aarch64, or universal)`);
}

export function sha256File(path) {
  const hash = createHash('sha256');
  hash.update(readFileSync(path));
  return hash.digest('hex').toUpperCase();
}

function machineFromBuffer(buffer) {
  if (buffer.length >= 64 && buffer[0] === 0x4d && buffer[1] === 0x5a) {
    const peOffset = buffer.readUInt32LE(0x3c);
    if (peOffset + 6 <= buffer.length && buffer.toString('ascii', peOffset, peOffset + 4) === 'PE\u0000\u0000') {
      const machine = buffer.readUInt16LE(peOffset + 4);
      if (machine === 0x8664) return ['x86_64'];
      if (machine === 0xaa64) return ['aarch64'];
      if (machine === 0x14c) return ['x86'];
      return [`pe-machine-0x${machine.toString(16)}`];
    }
  }
  if (buffer.length >= 64 && buffer[0] === 0x7f && buffer.toString('ascii', 1, 4) === 'ELF') {
    const littleEndian = buffer[5] !== 2;
    const machine = littleEndian ? buffer.readUInt16LE(0x12) : buffer.readUInt16BE(0x12);
    if (machine === 0x3e) return ['x86_64'];
    if (machine === 0xb7) return ['aarch64'];
    return [`elf-machine-0x${machine.toString(16)}`];
  }
  if (buffer.length >= 8) {
    const beam = buffer.readUInt32BE(0);
    if (beam === 0xcafebabe || beam === 0xcafebabf) {
      const count = buffer.readUInt32BE(4);
      const archs = [];
      for (let index = 0; index < Math.min(count, 16); index += 1) {
        const offset = 8 + index * 20;
        if (offset + 4 > buffer.length) break;
        const cpu = buffer.readUInt32BE(offset);
        if (cpu === 0x01000007) archs.push('x86_64');
        else if (cpu === 0x0100000c) archs.push('aarch64');
      }
      return archs.length ? archs : ['fat-macho-unknown'];
    }
    const swapped = buffer.readUInt32LE(0);
    if (beam === 0xfeedface || beam === 0xfeedfacf) {
      const cpu = buffer.readUInt32BE(4);
      if (cpu === 0x01000007) return ['x86_64'];
      if (cpu === 0x0100000c) return ['aarch64'];
      return [`macho-cpu-0x${cpu.toString(16)}`];
    }
    if (swapped === 0xfeedface || swapped === 0xfeedfacf) {
      const cpu = buffer.readUInt32LE(4);
      if (cpu === 0x01000007) return ['x86_64'];
      if (cpu === 0x0100000c) return ['aarch64'];
      return [`macho-cpu-0x${cpu.toString(16)}`];
    }
  }
  return ['unknown'];
}

/** Read the CPU architectures encoded in a PE/ELF/Mach-O binary. */
export function binaryArchs(path) {
  const header = Buffer.alloc(4096);
  const fd = openSync(path, 'r');
  let length;
  try {
    length = readSync(fd, header, 0, header.length, 0);
  } finally {
    closeSync(fd);
  }
  return machineFromBuffer(header.subarray(0, length));
}

/**
 * Checks whether child is strictly within parent path boundary.
 */
export function isPathInside(parent, child) {
  let realParent;
  let realChild;
  try {
    realParent = realpathSync(parent);
  } catch {
    realParent = resolve(parent);
  }
  try {
    realChild = realpathSync(child);
  } catch {
    realChild = resolve(child);
  }
  const rel = relative(realParent, realChild);
  return !rel.startsWith('..') && !isAbsolute(rel);
}

/**
 * Asserts that a package asset exists, is a regular file, and its realpath
 * does not escape the package root.
 */
export function assertSafePackageFile(root, filePath, label, problems) {
  if (!existsSync(filePath)) {
    problems.push(`missing ${label} (${relative(root, filePath)})`);
    return null;
  }
  let realPath;
  try {
    realPath = realpathSync(filePath);
  } catch (err) {
    problems.push(`${label} cannot be resolved (broken link or loop): ${err.message}`);
    return null;
  }
  if (!isPathInside(root, realPath)) {
    problems.push(`${label} escapes package root via link: ${filePath} -> ${realPath}`);
    return null;
  }
  let st;
  try {
    st = statSync(filePath);
  } catch (err) {
    problems.push(`failed to stat ${label}: ${err.message}`);
    return null;
  }
  if (!st.isFile()) {
    problems.push(`${label} is not a regular file: ${filePath}`);
    return null;
  }
  return { realPath, size: st.size };
}

/**
 * Traverses a directory safely:
 * - Uses lstat to inspect symbolic links and junctions.
 * - Resolves realpaths and strictly refuses to follow links that escape the package root.
 * - Tracks visited directories to prevent loops on circular links.
 */
export function safeWalk(root) {
  const files = [];
  const visitedDirs = new Set();
  let realRoot;
  try {
    realRoot = realpathSync(root);
  } catch {
    realRoot = resolve(root);
  }

  function visit(dir) {
    let realDir;
    try {
      realDir = realpathSync(dir);
    } catch {
      return;
    }
    if (visitedDirs.has(realDir)) return;
    visitedDirs.add(realDir);

    if (!isPathInside(realRoot, realDir)) {
      return;
    }

    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }

    for (const entry of entries) {
      const full = join(dir, entry.name);
      try {
        const lstat = lstatSync(full);
        if (lstat.isSymbolicLink()) {
          let targetReal;
          try {
            targetReal = realpathSync(full);
          } catch {
            continue;
          }
          if (!isPathInside(realRoot, targetReal)) {
            continue;
          }
          const targetStat = statSync(full);
          if (targetStat.isDirectory()) {
            visit(full);
          } else if (targetStat.isFile()) {
            files.push(full);
          }
        } else if (lstat.isDirectory()) {
          visit(full);
        } else if (lstat.isFile()) {
          files.push(full);
        }
      } catch {
        // file unreadable or gone
      }
    }
  }

  visit(root);
  return files;
}

function archMatches(actual, expected) {
  if (expected === 'universal') return actual.includes('x86_64') && actual.includes('aarch64');
  return actual.includes(expected);
}

function findLicenses(binDir, root, problems) {
  const candidates = [join(binDir, 'voice-licenses'), join(dirname(binDir), 'voice-licenses')];
  for (const dir of candidates) {
    if (!existsSync(dir)) continue;
    let realDir;
    try {
      realDir = realpathSync(dir);
    } catch (err) {
      problems.push(`voice-licenses link error: ${err.message}`);
      return REQUIRED_LICENSES.map((name) => ({ name, path: null }));
    }
    if (!isPathInside(root, realDir)) {
      problems.push(`voice-licenses directory escapes package root via link: ${dir} -> ${realDir}`);
      return REQUIRED_LICENSES.map((name) => ({ name, path: null }));
    }
    return REQUIRED_LICENSES.map((name) => ({ name, path: join(dir, name) }));
  }
  return REQUIRED_LICENSES.map((name) => ({ name, path: null }));
}

/**
 * Resolves the authoritative bin directory for the platform host executable.
 * Refuses arbitrary bins scattered in unrelated directories.
 */
export function findExpectedBin(root, platform) {
  if (platform === 'macos') {
    if (basename(root).endsWith('.app')) {
      return join(root, 'Contents', 'Resources', 'bin');
    }
    try {
      const entries = readdirSync(root, { withFileTypes: true });
      for (const entry of entries) {
        if (entry.name.endsWith('.app')) {
          const appPath = join(root, entry.name);
          try {
            const real = realpathSync(appPath);
            if (isPathInside(root, real)) {
              return join(appPath, 'Contents', 'Resources', 'bin');
            }
          } catch {}
        }
      }
    } catch {}
    return join(root, 'Contents', 'Resources', 'bin');
  }

  if (platform === 'windows') {
    // Check if Flowy.exe exists at root or app/
    if (existsSync(join(root, 'Flowy.exe'))) {
      return join(root, 'bin');
    }
    if (existsSync(join(root, 'app', 'Flowy.exe'))) {
      return join(root, 'app', 'bin');
    }
    return join(root, 'bin');
  }

  // Linux: standard layout under usr/lib/Flowy/bin or bin/
  const linuxCandidates = [
    join(root, 'usr', 'lib', 'Flowy', 'bin'),
    join(root, 'usr', 'lib', 'flowy', 'bin'),
    join(root, 'usr', 'share', 'flowy', 'bin'),
    join(root, 'usr', 'bin'),
    join(root, 'bin'),
  ];
  for (const cand of linuxCandidates) {
    if (existsSync(cand)) return cand;
  }
  return linuxCandidates[0];
}

/**
 * Verify one extracted package tree (the layout inside the installer/app/deb).
 * Returns human-readable records plus a non-empty problem list on failure.
 */
export function verifyExtractedTree(root, { platform, arch }) {
  const expected = normalizeArch(arch);
  const daemonName = platform === 'windows' ? 'voice-companion.exe' : 'voice-companion';
  const rgName = platform === 'windows' ? 'rg.exe' : 'rg';
  const problems = [];
  const records = [];

  const expectedBin = findExpectedBin(root, platform);
  const daemon = join(expectedBin, daemonName);

  const safeDaemon = assertSafePackageFile(root, daemon, daemonName, problems);
  if (!safeDaemon) {
    return { problems, records };
  }

  records.push(`daemon=${daemon}`);
  records.push(`daemon_sha256=${sha256File(daemon)}`);

  const archs = binaryArchs(daemon);
  records.push(`arch=${archs.join(',')} (expected ${expected})`);
  if (!archMatches(archs, expected)) {
    problems.push(`daemon architecture ${archs.join(',')} does not match expected ${expected}`);
  }

  const licenses = findLicenses(expectedBin, root, problems);
  for (const license of licenses) {
    if (!license.path) {
      problems.push(`missing voice license ${license.name} (expected voice-licenses/${license.name} next to ${daemonName})`);
      continue;
    }
    const safeLicense = assertSafePackageFile(root, license.path, `voice license ${license.name}`, problems);
    if (!safeLicense) {
      continue;
    }
    if (safeLicense.size === 0) {
      problems.push(`voice license ${license.name} is empty`);
      continue;
    }
    records.push(`license=${license.path} size=${safeLicense.size}`);
  }

  const rg = join(expectedBin, rgName);
  const safeRg = assertSafePackageFile(root, rg, `bundled ${rgName}`, problems);
  if (safeRg) {
    const rgArchs = binaryArchs(rg);
    records.push(`bundled_rg=${rg} arch=${rgArchs.join(',')}`);
    if (!archMatches(rgArchs, expected)) {
      problems.push(`bundled ${rgName} architecture ${rgArchs.join(',')} does not match expected ${expected}`);
    }
  }

  const legacyName = platform === 'windows' ? 'ai-mate.exe' : 'ai-mate';
  if (existsSync(join(expectedBin, legacyName))) {
    problems.push(`legacy ${legacyName} must not be packaged; it can mask a missing ${daemonName}`);
  }

  // Windows VC++ Runtime App-Local Deployment Check
  if (platform === 'windows') {
    for (const dll of REQUIRED_VC_REDIST_DLLS) {
      const dllPath = join(expectedBin, dll);
      const safeDll = assertSafePackageFile(root, dllPath, `required VC++ runtime DLL ${dll}`, problems);
      if (safeDll) {
        const dllArchs = binaryArchs(dllPath);
        records.push(`vc_redist=${dllPath} arch=${dllArchs.join(',')}`);
        if (!archMatches(dllArchs, expected)) {
          problems.push(`VC++ runtime DLL ${dll} architecture ${dllArchs.join(',')} does not match expected ${expected}`);
        }
      }
    }
  }

  return { problems, records };
}

function resolveSevenZip() {
  const candidates = [process.env.SEVEN_ZIP, '7z', '7za'];
  if (process.platform === 'win32') {
    candidates.push(
      join(process.env.ProgramFiles ?? 'C:/Program Files', '7-Zip', '7z.exe'),
      join(process.env['ProgramFiles(x86)'] ?? 'C:/Program Files (x86)', '7-Zip', '7z.exe'),
    );
  }
  for (const candidate of candidates) {
    if (!candidate) continue;
    const probe = spawnSync(candidate, ['i'], { stdio: 'ignore' });
    if (!probe.error && probe.status === 0) return candidate;
  }
  throw new UnverifiedError('7-Zip (7z) is required to unpack the installer');
}

export function resolveUnsquashfs() {
  const probe = spawnSync('unsquashfs', ['-v'], { encoding: 'utf8', windowsHide: true });
  if (probe.error || (probe.status !== 0 && !probe.stdout && !probe.stderr)) {
    throw new UnverifiedError('unsquashfs tool unavailable; install squashfs-tools');
  }
  return 'unsquashfs';
}

/**
 * Searches for a valid SquashFS 4.0 superblock in an AppImage binary.
 * AppImage type 2 binaries prepend an ELF runtime; unsquashfs requires -o <offset>.
 * Returns the byte offset of the SquashFS filesystem, or null if invalid / not found.
 */
export function findSquashfsOffset(filePath) {
  const fd = openSync(filePath, 'r');
  try {
    const stat = statSync(filePath);
    const fileSize = stat.size;
    if (fileSize < 96) return null;

    // Check if file starts immediately with SquashFS superblock at offset 0
    const header = Buffer.alloc(96);
    const bytesRead = readSync(fd, header, 0, 96, 0);
    if (bytesRead >= 96 && isSquashfsSuperblock(header, 0)) {
      return 0;
    }

    // Must start with ELF header
    if (header.subarray(0, 4).toString('ascii') !== '\x7fELF') {
      return null;
    }

    const chunkSize = 64 * 1024;
    const buffer = Buffer.alloc(chunkSize + 96);
    const maxSearch = Math.min(fileSize - 96, 2 * 1024 * 1024);

    for (let filePos = 0; filePos < maxSearch; filePos += chunkSize) {
      const readLen = Math.min(buffer.length, fileSize - filePos);
      if (readLen < 96) break;
      readSync(fd, buffer, 0, readLen, filePos);

      // Search for magic 0x73717368 (LE: 0x68, 0x73, 0x71, 0x73)
      for (let i = 0; i <= readLen - 96; i += 1) {
        if (
          buffer[i] === 0x68 &&
          buffer[i + 1] === 0x73 &&
          buffer[i + 2] === 0x71 &&
          buffer[i + 3] === 0x73
        ) {
          if (isSquashfsSuperblock(buffer, i)) {
            return filePos + i;
          }
        }
      }
    }
    return null;
  } finally {
    closeSync(fd);
  }
}

function isSquashfsSuperblock(buffer, offset) {
  if (offset + 96 > buffer.length) return false;
  // magic == 0x73717368
  const magic = buffer.readUInt32LE(offset);
  if (magic !== 0x73717368) return false;
  // version_major == 4, version_minor == 0
  const versionMajor = buffer.readUInt16LE(offset + 28);
  const versionMinor = buffer.readUInt16LE(offset + 30);
  if (versionMajor !== 4 || versionMinor !== 0) return false;
  // block_size is power of 2 between 4096 and 1048576
  const blockSize = buffer.readUInt32LE(offset + 12);
  if (blockSize < 4096 || blockSize > 1048576 || (blockSize & (blockSize - 1)) !== 0) {
    return false;
  }
  return true;
}

function run(command, args, label, timeoutMs = 120000) {
  const result = spawnSync(command, args, { encoding: 'utf8', windowsHide: true, timeout: timeoutMs });
  if (result.error) {
    if (result.error.code === 'ETIMEDOUT') {
      throw new Error(`${label} timed out after ${timeoutMs}ms`);
    }
    throw new UnverifiedError(`${label}: ${result.error.message}`);
  }
  if (result.status !== 0) {
    throw new Error(`${label} failed (exit ${result.status}): ${(result.stderr || result.stdout || '').trim().slice(0, 800)}`);
  }
  return result;
}

function extractWith7z(sevenZip, archive, destination) {
  run(sevenZip, ['x', '-y', `-o${destination}`, archive], `7z extract ${basename(archive)}`);
}

/** Pull nested archives (NSIS payload, updater archives, rpm cpio payloads) out of the first extraction. */
function extractNestedArchives(sevenZip, root, notes) {
  for (let depth = 0; depth < 3; depth += 1) {
    const archives = safeWalk(root).filter((path) => ['.7z', '.zip', '.cpio'].includes(extname(path).toLowerCase()));
    let extracted = 0;
    for (const archive of archives) {
      const destination = `${archive}.contents`;
      if (existsSync(destination)) continue;
      try {
        extractWith7z(sevenZip, archive, destination);
        extracted += 1;
      } catch (error) {
        notes.push(`nested archive skipped: ${basename(archive)} (${error.message.split('\n')[0]})`);
      }
    }
    if (!extracted) break;
  }
}

async function extractWindows(artifact, workDir) {
  const sevenZip = resolveSevenZip();
  const destination = join(workDir, 'windows-extract');
  extractWith7z(sevenZip, artifact, destination);
  const notes = [];
  extractNestedArchives(sevenZip, destination, notes);
  return { root: destination, notes };
}

async function extractMacos(artifact, workDir) {
  if (statSync(artifact).isDirectory()) return { root: artifact, notes: [] };
  if (process.platform !== 'darwin') {
    throw new UnverifiedError('DMG inspection requires macOS (hdiutil); passing an .app directory is supported everywhere');
  }
  const mount = join(workDir, 'dmg-mount');
  run('hdiutil', ['attach', '-nobrowse', '-readonly', '-mountpoint', mount, artifact], `mount ${basename(artifact)}`);
  return { root: mount, notes: [], mount };
}

async function extractLinux(artifact, workDir) {
  const extension = extname(artifact).toLowerCase();
  const destination = join(workDir, 'linux-extract');
  const notes = [];
  if (extension === '.deb') {
    run('dpkg-deb', ['-x', artifact, destination], `dpkg-deb -x ${basename(artifact)}`);
  } else if (extension === '.appimage') {
    const offset = findSquashfsOffset(artifact);
    if (offset === null) {
      throw new Error(`invalid AppImage: no valid SquashFS superblock found in ${basename(artifact)}`);
    }
    const unsquashfs = resolveUnsquashfs();
    const args = ['-n'];
    if (offset > 0) {
      args.push('-o', String(offset));
    }
    args.push('-d', destination, artifact);
    run(unsquashfs, args, `unsquashfs ${basename(artifact)} (offset ${offset})`);
  } else if (extension === '.rpm') {
    const sevenZip = resolveSevenZip();
    extractWith7z(sevenZip, artifact, destination);
    extractNestedArchives(sevenZip, destination, notes);
  } else {
    throw new UnverifiedError(`unsupported Linux artifact '${basename(artifact)}'`);
  }
  return { root: destination, notes };
}

function artifactsInDist(dist, platform) {
  const extensions =
    platform === 'windows' ? ['.exe'] :
    platform === 'macos' ? ['.dmg', '.app'] :
    ['.deb', '.appimage', '.rpm'];
  return readdirSync(dist)
    .map((name) => join(dist, name))
    .filter((path) => statSync(path).isDirectory() ? basename(path).endsWith('.app') : extensions.includes(extname(path).toLowerCase()))
    .sort();
}

function parseArgs(argv) {
  const options = { artifacts: [], dist: null, platform: null, arch: null };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--') continue;
    if (arg === '--artifact') options.artifacts.push(argv[++index]);
    else if (arg === '--dist') options.dist = argv[++index];
    else if (arg === '--platform') options.platform = argv[++index];
    else if (arg === '--arch') options.arch = argv[++index];
    else if (arg === '--help' || arg === '-h') options.help = true;
    else throw new Error(`unknown argument '${arg}'`);
  }
  return options;
}

async function verifyArtifact(artifact, platform, arch) {
  const workDir = await mkdtemp(join(tmpdir(), 'flowy-voice-package-'));
  let mounted;
  try {
    let extracted;
    if (statSync(artifact).isDirectory()) extracted = { root: artifact, notes: [] };
    else if (platform === 'windows') extracted = await extractWindows(artifact, workDir);
    else if (platform === 'macos') extracted = await extractMacos(artifact, workDir);
    else extracted = await extractLinux(artifact, workDir);
    mounted = extracted.mount;
    const record = [`artifact=${resolve(artifact)}`, `artifact_sha256=${statSync(artifact).isDirectory() ? '(directory)' : sha256File(artifact)}`];
    for (const note of extracted.notes) record.push(`note=${note}`);
    const { problems, records } = verifyExtractedTree(extracted.root, { platform, arch });
    for (const line of records) record.push(line);
    if (problems.length) {
      const summary = `FAIL ${artifact}:\n  - ` + problems.join('\n  - ');
      throw new Error(summary);
    }
    console.log(record.join('\n'));
    console.log(`VERIFIED ${artifact}`);
  } finally {
    if (mounted) {
      run('hdiutil', ['detach', mounted, '-force'], `detach ${mounted}`);
    }
    await rm(workDir, { recursive: true, force: true });
  }
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help || (!options.artifacts.length && !options.dist) || !options.platform || !options.arch) {
    console.log(`Usage: bun scripts/verify-voice-release-package.mjs --platform <windows|macos|linux> --arch <x64|arm64|universal> (--artifact <path> | --dist <dir>)`);
    process.exit(options.help ? 0 : 2);
  }

  const artifacts = [...options.artifacts];
  if (options.dist) {
    if (!existsSync(options.dist)) {
      throw new Error(`distribution directory does not exist: ${options.dist}`);
    }
    artifacts.push(...artifactsInDist(options.dist, options.platform));
  }
  if (!artifacts.length) {
    throw new Error(`no release artifacts found for platform ${options.platform}`);
  }

  for (const artifact of artifacts) {
    await verifyArtifact(artifact, options.platform, options.arch);
  }
}

if (import.meta.main) {
  main().catch((error) => {
    if (error instanceof UnverifiedError) {
      console.error(`UNVERIFIED: ${error.message}`);
      process.exit(2);
    }
    console.error(error.message);
    process.exit(1);
  });
}
