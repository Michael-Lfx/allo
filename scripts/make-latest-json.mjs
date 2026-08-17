#!/usr/bin/env bun
/**
 * make-latest-json — 生成 Tauri 自动更新清单（按平台独立渠道）。
 *
 *   bun run make:latest --host modelscope --collect
 *   bun run make:latest --host modelscope --channel windows --collect
 *   bun run make:latest --version 0.4.2 --host modelscope --channel macos --collect
 *
 * ModelScope 三端独立渠道（版本可长期不同步）：
 *   allo/channels/windows|macos|linux/latest.json
 *   allo/{windows|macos|linux}/v{version}/...
 *
 * 旧的共享 alpha 清单已废弃；正式发版构建必须叠加
 * apps/desktop/tauri.channel.{windows|macos|linux}.conf.json。
 *
 * 单一真源 = 根 Cargo.toml [workspace.package].version（打在本 commit 上的版本）。
 * 纯 node:fs，无第三方依赖。
 */
import { readFileSync, writeFileSync, existsSync, readdirSync, statSync, mkdirSync, copyFileSync } from 'node:fs';
import { dirname, join, basename, isAbsolute, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const DEFAULT_REPO = 'nomifun/nomifun-tauri';
const DEFAULT_MS_REPO = 'flowy2025/flowyaipc';
const DEFAULT_MS_PREFIX = 'allo';
const PRODUCT = 'Flowy';
const ALL_KEYS = ['windows-x86_64', 'windows-aarch64', 'darwin-x86_64', 'darwin-aarch64', 'linux-x86_64', 'linux-aarch64'];
const ALL_KEYS_SET = new Set(ALL_KEYS);
const PLATFORM_CHANNELS = ['windows', 'macos', 'linux'];
const CHANNEL_KEYS = {
  windows: ['windows-x86_64', 'windows-aarch64'],
  macos: ['darwin-x86_64', 'darwin-aarch64'],
  linux: ['linux-x86_64', 'linux-aarch64'],
};

const rel = (p) => (p.startsWith(ROOT) ? p.slice(ROOT.length + 1) : p);

function inferHostChannel() {
  if (process.platform === 'win32') return 'windows';
  if (process.platform === 'darwin') return 'macos';
  return 'linux';
}

function assertPlatformChannel(channel) {
  if (!PLATFORM_CHANNELS.includes(channel)) {
    throw new Error(`--channel 必须是 ${PLATFORM_CHANNELS.join('|')}（已废弃 alpha 共享渠道），收到: ${channel}`);
  }
}

/** Reject legacy NomiFun_* names; require Flowy product prefix per platform key. */
function assertFlowyArtifactName(key, name) {
  if (!ALL_KEYS_SET.has(key)) {
    throw new Error(`未知平台键 ${key}（允许: ${ALL_KEYS.join(', ')}）`);
  }
  if (/^NomiFun/i.test(name)) {
    throw new Error(
      `${key} 产物仍为遗留名 ${name}。productName 已是 Flowy，请用当前配置重建（期望 Flowy_* / Flowy.app.*）。`,
    );
  }
  const lower = name.toLowerCase();
  if (key.startsWith('windows-')) {
    if (!name.startsWith(`${PRODUCT}_`) || !lower.endsWith('-setup.exe')) {
      throw new Error(
        `${key} 产物名非法: ${name}（期望 ${PRODUCT}_{version}_x64-setup.exe 或 …_aarch64-setup.exe）`,
      );
    }
    return;
  }
  if (key.startsWith('darwin-')) {
    if (
      name === `${PRODUCT}.app.tar.gz` ||
      (name.startsWith(`${PRODUCT}_`) && lower.endsWith('.app.tar.gz'))
    ) {
      return;
    }
    throw new Error(
      `${key} 产物名非法: ${name}（期望 ${PRODUCT}.app.tar.gz 或 ${PRODUCT}_{version}_*.app.tar.gz）`,
    );
  }
  if (key.startsWith('linux-')) {
    if (!name.startsWith(`${PRODUCT}_`) || !lower.endsWith('.appimage')) {
      throw new Error(
        `${key} 产物名非法: ${name}（期望 ${PRODUCT}_{version}_amd64.AppImage / _x86_64.AppImage / _aarch64.AppImage）`,
      );
    }
    return;
  }
  throw new Error(`未处理的平台键: ${key}`);
}

function buildChannelYml(manifest, channel) {
  const lines = [
    `# ${channel} channel pointer — updated by make:latest / upload-modelscope-release.py.`,
    `# Clients read allo/channels/${channel}/latest.json, not this file directly.`,
    `version: "${manifest.version || ''}"`,
    `channel: ${channel}`,
    `pub_date: "${manifest.pub_date || ''}"`,
    `manifest: channels/${channel}/latest.json`,
  ];
  const notes = typeof manifest.notes === 'string' ? manifest.notes.trim() : '';
  if (notes) {
    lines.push('notes: |');
    for (const noteLine of notes.split(/\r?\n/)) lines.push(`  ${noteLine}`);
  } else {
    lines.push('notes: ""');
  }
  return lines.join('\n') + '\n';
}

// ── 参数解析（--flag value / 无值开关返回 true） ──────────────────────────────
const argv = process.argv.slice(2);
function flag(name, fallback = undefined) {
  const i = argv.indexOf(`--${name}`);
  if (i === -1) return fallback;
  const next = argv[i + 1];
  return next && !next.startsWith('--') ? next : true;
}

const host = flag('host', 'github');
const msRepo = flag('ms-repo', DEFAULT_MS_REPO);
const msPrefix = flag('ms-prefix', DEFAULT_MS_PREFIX);
const channelArg = flag('channel', inferHostChannel());
const msChannel = typeof channelArg === 'string' ? channelArg : inferHostChannel();
try {
  assertPlatformChannel(msChannel);
} catch (err) {
  console.error(`✗ ${err.message}`);
  process.exit(1);
}
const channelKeySet = new Set(CHANNEL_KEYS[msChannel]);
const repo = flag('repo', DEFAULT_REPO);
// ModelScope: one manifest per OS channel. GitHub Releases: one shared latest.json
// that can accumulate platforms across native builders.
const defaultOut =
  host === 'modelscope'
    ? join(ROOT, 'apps/desktop/updater', `latest.${msChannel}.json`)
    : join(ROOT, 'apps/desktop/updater', 'latest.json');
const out = flag('out', defaultOut);
const channelYmlPath = join(ROOT, 'apps/desktop/updater', `channel.${msChannel}.yml`);
const collect = flag('collect', false) === true;
const targetDirArg = flag('target-dir', join(ROOT, 'target'));
if (typeof targetDirArg !== 'string') {
  console.error('✗ --target-dir 需要目录路径。');
  process.exit(1);
}
const TARGET = isAbsolute(targetDirArg) ? targetDirArg : resolve(ROOT, targetDirArg);
const version = flag('version') || readWorkspaceVersion();
const notesArg = flag('notes');
const notesFile = flag('notes-file');
const notesFromFile = typeof notesFile === 'string' && existsSync(notesFile) ? readFileSync(notesFile, 'utf8').trim() : null;
let notes = typeof notesArg === 'string' ? notesArg : notesFromFile;
const distDir = join(ROOT, 'dist/desktop');

// 单一真源版本号：根 Cargo.toml 的 [workspace.package].version。
function readWorkspaceVersion() {
  const lines = readFileSync(join(ROOT, 'Cargo.toml'), 'utf8').split('\n');
  let inSection = false;
  for (const line of lines) {
    const t = line.trim();
    if (t.startsWith('[')) {
      inSection = t === '[workspace.package]';
      continue;
    }
    if (inSection) {
      const m = line.match(/^\s*version\s*=\s*"([^"]+)"/);
      if (m) return m[1];
    }
  }
  console.error('✗ 无法从根 Cargo.toml 读取 [workspace.package].version');
  process.exit(1);
}

function readChangelogNotes(version) {
  const p = join(ROOT, 'CHANGELOG.md');
  if (!existsSync(p)) return null;
  const lines = readFileSync(p, 'utf8').split('\n');
  const heads = [];
  for (let i = 0; i < lines.length; i++) {
    if (/^##\s+/.test(lines[i])) heads.push(i);
  }
  if (heads.length === 0) return null;
  const isUnreleased = (i) => /^##\s+unreleased\b/i.test(lines[i]);
  const namesVersion = (i) => version && lines[i].includes(version);
  let start = heads.find(namesVersion);
  if (start === undefined) start = heads.find((i) => !isUnreleased(i));
  if (start === undefined) return null;
  const body = [];
  for (let i = start + 1; i < lines.length; i++) {
    if (/^##\s+/.test(lines[i])) break;
    body.push(lines[i]);
  }
  const text = body.join('\n').trim();
  return text || null;
}

function platformKeysFor(triple) {
  if (triple.includes('apple-darwin')) {
    if (triple.includes('universal')) return ['darwin-x86_64', 'darwin-aarch64'];
    return [triple.includes('aarch64') ? 'darwin-aarch64' : 'darwin-x86_64'];
  }
  if (triple.includes('windows')) return [triple.includes('aarch64') ? 'windows-aarch64' : 'windows-x86_64'];
  if (triple.includes('linux')) return [triple.includes('aarch64') ? 'linux-aarch64' : 'linux-x86_64'];
  return [];
}

function platformFolderForKey(key) {
  if (key.startsWith('windows-')) return 'windows';
  if (key.startsWith('darwin-')) return 'macos';
  if (key.startsWith('linux-')) return 'linux';
  return null;
}

function artifactDownloadUrl(name, platformKey) {
  if (host === 'modelscope') {
    const versionTag = version.startsWith('v') ? version : `v${version}`;
    const folder = platformFolderForKey(platformKey);
    if (!folder) {
      throw new Error(`unsupported platform key for ModelScope URL: ${platformKey}`);
    }
    const filePath = `${msPrefix}/${folder}/${versionTag}/${name}`;
    return `https://modelscope.cn/api/v1/models/${msRepo}/repo?Revision=master&FilePath=${filePath}`;
  }
  return `https://github.com/${repo}/releases/download/v${version}/${name}`;
}

function hostTriple() {
  const arch = process.arch === 'arm64' ? 'aarch64' : process.arch === 'x64' ? 'x86_64' : process.arch;
  if (process.platform === 'darwin') return `${arch}-apple-darwin`;
  if (process.platform === 'win32') return `${arch}-pc-windows-msvc`;
  return `${arch}-unknown-linux-gnu`;
}

function listDirs(p) {
  if (!existsSync(p)) return [];
  return readdirSync(p).filter((e) => statSync(join(p, e)).isDirectory());
}

function findSigs(bundleDir) {
  const found = [];
  const walk = (dir) => {
    for (const e of readdirSync(dir)) {
      const full = join(dir, e);
      if (statSync(full).isDirectory()) walk(full);
      else if (e.endsWith('.sig')) {
        const artifact = full.slice(0, -4);
        if (existsSync(artifact)) found.push({ artifact, sig: full });
      }
    }
  };
  walk(bundleDir);
  return found;
}

function artifactPriority(key, artifact) {
  const name = basename(artifact).toLowerCase();
  const order = key.startsWith('linux-')
    ? ['.appimage', '.deb', '.rpm']
    : key.startsWith('windows-')
      ? ['-setup.exe', '.exe', '.msi']
      : key.startsWith('darwin-')
        ? ['.app.tar.gz', '.tar.gz', '.dmg']
        : [];

  const index = order.findIndex((suffix) => name.endsWith(suffix));
  return index === -1 ? order.length : index;
}

function compareCandidates(key, a, b) {
  const byPriority = artifactPriority(key, a.artifact) - artifactPriority(key, b.artifact);
  if (byPriority !== 0) return byPriority;
  return basename(a.artifact).localeCompare(basename(b.artifact), 'en');
}

const collected = {}; // platformKey -> { url, signature, artifact, sig }

function collectCandidate(key, candidate) {
  const current = collected[key];
  if (!current) {
    collected[key] = candidate;
    return;
  }

  const nextIsBetter = compareCandidates(key, candidate, current) < 0;
  const kept = nextIsBetter ? candidate : current;
  const ignored = nextIsBetter ? current : candidate;
  console.warn(
    `  ! ${key} 多个候选产物，选择 ${basename(kept.artifact)} 作为 updater 包，仍会上传 ${basename(ignored.artifact)}。`,
  );
  collected[key] = kept;
}

// ── 扫描 target/ ────────────────────────────────────────────────────────────
if (!existsSync(TARGET)) {
  console.error(`✗ 找不到 target/（${rel(TARGET)}）。先构建更新产物：bun run build:updater`);
  process.exit(1);
}

const bundleDirs = [];
const directDefault = join(TARGET, 'release', 'bundle');
if (existsSync(directDefault)) bundleDirs.push({ dir: directDefault, triple: hostTriple() });
for (const entry of listDirs(TARGET)) {
  if (entry === 'release' || entry === 'debug') continue;
  const nested = join(TARGET, entry, 'release', 'bundle');
  if (existsSync(nested)) bundleDirs.push({ dir: nested, triple: entry });
}

const uploads = new Set();
for (const { dir, triple } of bundleDirs) {
  let keys = platformKeysFor(triple);
  if (host === 'modelscope') {
    keys = keys.filter((key) => channelKeySet.has(key));
  }
  if (keys.length === 0) {
    console.warn(`  ! 跳过与当前托管/渠道无关的 triple: ${triple}`);
    continue;
  }
  for (const { artifact, sig } of findSigs(dir)) {
    const name = basename(artifact);
    const signature = readFileSync(sig, 'utf8').trim();
    for (const key of keys) {
      const url = artifactDownloadUrl(name, key);
      collectCandidate(key, { url, signature, artifact, sig });
    }
    uploads.add(artifact);
    uploads.add(sig);
  }
}

const foundKeys = Object.keys(collected);
if (foundKeys.length === 0) {
  console.error(`✗ 在 target/ 下没找到渠道 ${msChannel} 的更新产物（*.sig）。先构建带更新签名的产物：`);
  console.error(
    '    macOS:   bun run build:mac arm --config apps/desktop/tauri.updater.conf.json --config apps/desktop/tauri.channel.macos.conf.json',
  );
  console.error(
    '    Windows: bun run build:win --config apps/desktop/tauri.updater.conf.json --config apps/desktop/tauri.channel.windows.conf.json',
  );
  console.error(
    '    Linux:   bun run build:linux --config apps/desktop/tauri.updater.conf.json --config apps/desktop/tauri.channel.linux.conf.json',
  );
  process.exit(1);
}

for (const key of foundKeys) {
  const name = basename(collected[key].artifact);
  try {
    assertFlowyArtifactName(key, name);
  } catch (err) {
    console.error(`✗ ${err.message}`);
    process.exit(1);
  }
}

// ── 合并既有清单。ModelScope 禁止跨渠道；GitHub 可跨平台累积同版本条目。 ──
const manifest = { version, notes: '', pub_date: new Date().toISOString(), platforms: {} };
if (existsSync(out)) {
  try {
    const prev = JSON.parse(readFileSync(out, 'utf8'));
    if (prev.version === version) {
      if (!notes && typeof prev.notes === 'string' && prev.notes.trim()) notes = prev.notes.trim();
      for (const [k, v] of Object.entries(prev.platforms || {})) {
        if (!ALL_KEYS_SET.has(k)) {
          console.warn(`  ! 丢弃未知平台键: ${k}`);
          continue;
        }
        if (host === 'modelscope' && !channelKeySet.has(k)) {
          console.warn(`  ! 丢弃非本渠道平台键: ${k}`);
          continue;
        }
        const placeholder = !v?.signature || v.signature.includes('<<') || String(v.url).includes('REPLACE-WITH');
        if (placeholder) continue;
        const urlName = basename(String(v.url || '').split('?')[0]);
        if (urlName && /^NomiFun/i.test(urlName)) {
          console.warn(`  ! 丢弃遗留产物名条目 ${k}: ${urlName}（需用 Flowy_* 重建）`);
          continue;
        }
        if (host === 'modelscope' && !String(v.url).includes('modelscope.cn')) continue;
        if (host === 'github' && String(v.url).includes('modelscope.cn')) continue;
        manifest.platforms[k] = v;
      }
    } else if (prev.version) {
      console.warn(`  ! 既有清单版本 ${prev.version} ≠ 本次 ${version}，丢弃旧平台条目，重建清单。`);
    }
  } catch {
    console.warn(`  ! 既有 ${rel(out)} 解析失败，将重新生成。`);
  }
}
manifest.notes = notes || readChangelogNotes(version) || `Flowy v${version}`;
for (const key of foundKeys) {
  manifest.platforms[key] = { signature: collected[key].signature, url: collected[key].url };
}

mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, JSON.stringify(manifest, null, 2) + '\n');

const channelYml = buildChannelYml(manifest, msChannel);
if (host === 'modelscope') {
  writeFileSync(channelYmlPath, channelYml);
}

if (collect) {
  mkdirSync(distDir, { recursive: true });
  for (const f of uploads) copyFileSync(f, join(distDir, basename(f)));
  writeFileSync(join(distDir, 'latest.json'), JSON.stringify(manifest, null, 2) + '\n');
  writeFileSync(join(distDir, 'channel.yml'), channelYml);
}

const line = '━'.repeat(66);
console.log(line);
console.log(`✓ latest.json 已写入: ${rel(out)}`);
const hostLabel =
  host === 'modelscope'
    ? `ModelScope ${msRepo}/${msPrefix}/channels/${msChannel}`
    : `GitHub ${repo}`;
console.log(`  版本: ${version}    渠道: ${msChannel}    托管: ${hostLabel}`);
console.log('  平台条目:');
const reportKeys = host === 'modelscope' ? CHANNEL_KEYS[msChannel] : ALL_KEYS;
for (const key of reportKeys) {
  const here = foundKeys.includes(key);
  const mark = here ? '✓ 本次填入' : manifest.platforms[key] ? '· 沿用既有' : '✗ 缺失';
  console.log(`    ${key.padEnd(16)} ${mark}`);
}
console.log('');
const uploadHint =
  host === 'modelscope'
    ? `待上传到 ModelScope（python scripts/upload-modelscope-release.py --dist-dir dist/desktop/ --channel ${msChannel}）的本机产物:`
    : `待上传到 GitHub Release（tag v${version}）的本机产物:`;
console.log(`  ${uploadHint}`);
for (const f of uploads) console.log(`    ${rel(f)}`);
console.log(`    ${rel(out)}`);
if (collect) console.log(`  已拷贝到: ${rel(distDir)}/`);
if (host === 'modelscope') console.log(`  channel.yml: ${rel(channelYmlPath)}`);
console.log(line);
