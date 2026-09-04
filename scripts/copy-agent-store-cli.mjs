#!/usr/bin/env bun
/**
 * copy-agent-store-cli — 复制 release 构建的 agent-store.exe 到 cli/ 交付目录。
 *
 *   bun scripts/copy-agent-store-cli.mjs
 *
 * 纯 node:fs，无第三方依赖（与 scripts/help.mjs 风格一致）。
 * 找不到产物时打印构建指引并以退出码 1 结束。
 */
import { copyFileSync, existsSync, mkdirSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const SOURCE = join(ROOT, 'target', 'release', 'agent-store.exe');
const EXE_NAME = process.platform === 'win32' ? 'agent-store.exe' : 'agent-store';
const DEST_DIR = join(ROOT, 'cli');
const DEST = join(DEST_DIR, EXE_NAME);

if (!existsSync(SOURCE)) {
  console.error(
    `[copy-agent-store-cli] 未找到产物: ${SOURCE}\n` +
      `请先构建:  bun run agent-store:build\n` +
      `（即 bun --cwd web run build && cargo build --release -p agent-store --features static-webui）`
  );
  process.exit(1);
}

mkdirSync(DEST_DIR, { recursive: true });
copyFileSync(SOURCE, DEST);

const sizeMb = (statSync(DEST).size / 1024 / 1024).toFixed(1);
console.log(`[copy-agent-store-cli] 已复制到 ${DEST} (${sizeMb} MB)`);