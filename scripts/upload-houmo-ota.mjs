#!/usr/bin/env bun
/**
 * upload:houmo-ota — 生成并上传“后摩版”桌面 OTA 到 ModelScope 的 `allo/houmo` 渠道。
 *
 *   bun run upload:houmo-ota -- --channel windows --dry-run
 *   bun run upload:houmo-ota -- --channel windows
 *
 * 前缀写死为 `allo/houmo`，不接受改成公开渠道 `allo`。两步都复用现有脚本：
 *
 *   1) make:latest        --host modelscope --ms-prefix allo/houmo --channel <os> --collect
 *   2) run-upload-modelscope.mjs --prefix allo/houmo
 *
 * 上传前校验清单里每个平台 URL 与即将使用的远端路径都含 `houmo`，任一缺失即中止，
 * 不会调用上传脚本。GitHub 镜像不在这支脚本里（客户端地址由编译期前缀生成）。
 */
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');

/** 后摩渠道前缀：固定值，禁止改成公开渠道 `allo`。 */
export const HOUMO_PREFIX = 'allo/houmo';

export const PLATFORM_CHANNELS = ['windows', 'macos', 'linux'];

/** 远端渠道指针路径：allo/houmo/channels/{channel}/latest.json。 */
export function houmoManifestRemotePath(channel) {
  return `${HOUMO_PREFIX}/channels/${channel}/latest.json`;
}

/** 远端安装包路径：allo/houmo/{channel}/v{version}/{filename}。 */
export function houmoArtifactRemotePath(channel, version, filename) {
  const trimmed = String(version).trim().replace(/^v/, '');
  return `${HOUMO_PREFIX}/${channel}/v${trimmed}/${filename}`;
}

/** 缺 `houmo` 就抛错，返回原路径便于内联使用。 */
export function assertHoumoPath(path, label = '路径') {
  if (typeof path !== 'string' || path.length === 0 || !path.includes('houmo')) {
    throw new Error(`${label}必须包含 houmo 前缀，收到: ${String(path)}`);
  }
  return path;
}

export function inferHostChannel() {
  if (process.platform === 'win32') return 'windows';
  if (process.platform === 'darwin') return 'macos';
  return 'linux';
}

export function resolveChannel(value) {
  const channel = typeof value === 'string' && value.length > 0 ? value : inferHostChannel();
  if (!PLATFORM_CHANNELS.includes(channel)) {
    throw new Error(`--channel 必须是 ${PLATFORM_CHANNELS.join('|')}，收到: ${channel}`);
  }
  return channel;
}

function folderForKey(key) {
  if (key.startsWith('windows-')) return 'windows';
  if (key.startsWith('darwin-')) return 'macos';
  if (key.startsWith('linux-')) return 'linux';
  return null;
}

function artifactFilenameFromUrl(url) {
  const match = /FilePath=([^&]+)/.exec(url);
  const raw = match ? decodeURIComponent(match[1]) : url.split('?')[0];
  return raw.split('/').filter(Boolean).pop() || '';
}

/** 返回清单里所有不含 `houmo` 的平台 URL（空数组 = 合规）。 */
export function houmoManifestViolations(manifest) {
  const violations = [];
  for (const [key, entry] of Object.entries(manifest?.platforms || {})) {
    const url = String(entry?.url || '');
    if (!url.includes('houmo')) violations.push(`${key}: ${url}`);
  }
  return violations;
}

/**
 * 校验清单 URL 与即将使用的远端路径（渠道指针 + 每个平台安装包）都含 `houmo`。
 * 任一不合规即抛错，调用方不得继续上传。
 */
export function assertManifestHoumo(manifest, channel) {
  const violations = houmoManifestViolations(manifest);
  if (violations.length > 0) {
    throw new Error(
      `清单存在不含 houmo 的平台 URL：\n  - ${violations.join('\n  - ')}\n` +
        '拒绝上传，避免把后摩包推到公开渠道。',
    );
  }
  assertHoumoPath(houmoManifestRemotePath(channel), '渠道指针');
  for (const [key, entry] of Object.entries(manifest?.platforms || {})) {
    const folder = folderForKey(key);
    const filename = artifactFilenameFromUrl(String(entry?.url || ''));
    if (folder && filename) {
      assertHoumoPath(
        houmoArtifactRemotePath(folder, manifest.version, filename),
        `${key} 安装包`,
      );
    }
  }
  return manifest;
}

function parseArgs(argv) {
  const opts = { channel: null, distDir: join(ROOT, 'dist/desktop'), passthrough: [] };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--channel') {
      opts.channel = argv[++i];
    } else if (arg === '--dist-dir') {
      const value = argv[++i];
      if (!value) throw new Error('--dist-dir 需要目录路径');
      opts.distDir = isAbsolute(value) ? value : resolve(ROOT, value);
    } else if (['--dry-run', '--artifacts-only', '--manifest-only', '--merge-remote'].includes(arg)) {
      opts.passthrough.push(arg);
    } else {
      throw new Error(`未知参数: ${arg}（可用: --channel --dist-dir --dry-run --artifacts-only --manifest-only --merge-remote）`);
    }
  }
  return opts;
}

function runBun(args) {
  const result = spawnSync(process.execPath, args, { cwd: ROOT, stdio: 'inherit', env: process.env });
  if (result.error) throw new Error(`启动失败: ${result.error.message}`);
  if (result.status !== 0) throw new Error(`命令失败（退出码 ${result.status}）: ${args.join(' ')}`);
}

function main() {
  let opts;
  try {
    opts = parseArgs(process.argv.slice(2));
  } catch (err) {
    console.error(`✗ ${err.message}`);
    process.exit(1);
  }

  let channel;
  try {
    channel = resolveChannel(opts.channel);
  } catch (err) {
    console.error(`✗ ${err.message}`);
    process.exit(1);
  }

  console.log(`后摩版 OTA 上传：渠道 ${channel}，前缀 ${HOUMO_PREFIX}`);
  try {
    runBun([
      join(ROOT, 'scripts', 'make-latest-json.mjs'),
      '--host',
      'modelscope',
      '--ms-prefix',
      HOUMO_PREFIX,
      '--channel',
      channel,
      '--collect',
    ]);

    const latestPath = join(opts.distDir, 'latest.json');
    if (!existsSync(latestPath)) {
      throw new Error(`找不到清单 ${latestPath}`);
    }
    const manifest = JSON.parse(readFileSync(latestPath, 'utf8'));
    assertManifestHoumo(manifest, channel);
    console.log(`✓ 清单与远端路径均已锁定 ${HOUMO_PREFIX}`);

    runBun([
      join(ROOT, 'scripts', 'run-upload-modelscope.mjs'),
      '--dist-dir',
      opts.distDir,
      '--prefix',
      HOUMO_PREFIX,
      '--channel',
      channel,
      ...opts.passthrough,
    ]);
  } catch (err) {
    console.error(`✗ ${err.message}`);
    process.exit(1);
  }
}

if (import.meta.main) {
  main();
}
