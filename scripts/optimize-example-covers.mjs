#!/usr/bin/env node
/**
 * Recompress and validate the example covers bundled with the home screen.
 *
 * Usage:
 *   bun run optimize:example-covers          # rewrite only when smaller
 *   bun run check:example-covers             # fail when dimensions/size drift
 *
 * Requires ImageMagick (`magick` or `convert`) on PATH. It is a packaging-time
 * helper, not part of the runtime build, so CI without ImageMagick can skip it.
 */
import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync, renameSync, rmSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const ASSETS = join(ROOT, 'ui/src/renderer/pages/guid/components/assets');
const GROUPS = [
  { dir: 'build', quality: 74, maxBytes: 24_000 },
  { dir: 'image-examples', quality: 74, maxBytes: 32_000 },
  { dir: '.', quality: 80, maxBytes: 30_000, prefix: 'showcase-' },
];
const MAX_WIDTH = 512;
const MAX_HEIGHT = 288;
const CHECK = process.argv.includes('--check');

function findImageMagick() {
  for (const command of ['magick', 'convert']) {
    const result = spawnSync(command, ['-version'], { encoding: 'utf8' });
    if (result.status === 0) return command;
  }
  return null;
}

function targets() {
  const files = [];
  for (const group of GROUPS) {
    const dir = join(ASSETS, group.dir);
    if (!existsSync(dir)) continue;
    for (const name of readdirSync(dir)) {
      if (!name.endsWith('.webp')) continue;
      if (group.prefix && !name.startsWith(group.prefix)) continue;
      files.push({ file: join(dir, name), ...group });
    }
  }
  return files;
}

function identify(command, file) {
  const result = spawnSync(command, ['identify', '-format', '%w %h %B', file], {
    encoding: 'utf8',
  });
  if (result.status !== 0) throw new Error(`${file}: identify failed`);
  const [width, height, size] = result.stdout.trim().split(/\s+/);
  return { width: Number(width), height: Number(height), size: Number(size) };
}

const command = findImageMagick();
if (!command) {
  console.error('ImageMagick is required (magick or convert on PATH).');
  process.exit(1);
}

let failed = false;
let beforeBytes = 0;
let afterBytes = 0;
for (const { file, quality, maxBytes } of targets()) {
  const old = identify(command, file);
  beforeBytes += old.size;
  if (CHECK) {
    if (old.width > MAX_WIDTH || old.height > MAX_HEIGHT) {
      console.error(`${file}: ${old.width}x${old.height} exceeds ${MAX_WIDTH}x${MAX_HEIGHT}`);
      failed = true;
    }
    if (old.size > maxBytes) {
      console.error(`${file}: ${old.size}B exceeds ${maxBytes}B`);
      failed = true;
    }
    afterBytes += old.size;
    continue;
  }

  const temp = `${file}.optimized.webp`;
  const result = spawnSync(command, [
    file,
    '-resize',
    `${MAX_WIDTH}x`,
    '-strip',
    '-quality',
    String(quality),
    '-define',
    'webp:method=6',
    temp,
  ]);
  if (result.status !== 0) {
    rmSync(temp, { force: true });
    throw new Error(`${file}: compression failed`);
  }
  const optimized = statSync(temp).size;
  if (optimized < old.size) {
    renameSync(temp, file);
    afterBytes += optimized;
  } else {
    rmSync(temp, { force: true });
    afterBytes += old.size;
  }
}

if (CHECK) {
  if (failed) process.exit(1);
  console.log(`example covers OK: ${afterBytes} bytes`);
} else {
  console.log(`example covers: ${beforeBytes} -> ${afterBytes} bytes`);
}
