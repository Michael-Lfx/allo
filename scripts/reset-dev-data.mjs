#!/usr/bin/env bun
/**
 * Arm an explicit v3 factory reset for the dev-channel data dir.
 *
 * The v3 bootstrap refuses to open a database whose sqlx migration lineage
 * (version + SHA-384 checksum) does not match the embedded migrations, and the
 * one automatic legacy retirement is consumed once per installation. When a
 * migration file was edited after a local database already applied it (a
 * common hazard after merging branches), startup fails with a Conflict that
 * demands an explicit factory reset. This script writes that request file —
 * the same control-plane request `POST /api/system/factory-reset` arms — so
 * the next boot retires the incompatible dataset and starts a fresh v3 one.
 *
 * The old dataset is quarantined under `<data_dir>/retired-datasets/`, never
 * deleted; recover anything you need from there before restarting.
 *
 * SAFETY: quit the desktop app / `bun run dev` / `nomicore` first. The request
 * is consumed on the NEXT boot; a running instance keeps serving the old
 * dataset and would ignore it.
 *
 * Usage: bun scripts/reset-dev-data.mjs [--data-dir <path>] [--yes]
 *   --data-dir  override the dev data dir (default: %LOCALAPPDATA%/Flowy/Nomi-dev
 *               or the sibling per-OS location; NOMIFUN_DATA_DIR/FLOWY_DATA_DIR win)
 *   --yes       skip the confirmation prompt
 */
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { homedir, platform } from 'node:os';
import { dirname, join } from 'node:path';
import { randomBytes } from 'node:crypto';
import { createInterface } from 'node:readline';

// Must match `nomifun_common::factory_reset::V3_DATASET_RESET_REQUEST_FILE`.
const RESET_REQUEST_FILE = '.dataset-v3-reset.request.json';
const DIR_CONFIG_FILE = 'dir-config.json';
const DATABASE_LEAVES = ['flowy-backend.db', 'nomifun-backend.db'];

/** Mirror `nomifun_common::storage_paths::default_data_dir` (channel `dev`). */
function devDataDir() {
  const fromEnv =
    process.env.NOMIFUN_DATA_DIR || process.env.FLOWY_DATA_DIR;
  if (fromEnv) {
    return fromEnv;
  }
  const home = homedir();
  const base =
    platform() === 'win32'
      ? process.env.LOCALAPPDATA ?? join(home, 'AppData', 'Local')
      : platform() === 'darwin'
        ? join(home, 'Library', 'Application Support')
        : process.env.XDG_DATA_HOME ?? join(home, '.local', 'share');
  return join(base, 'Flowy', 'Nomi-dev');
}

/** RFC 9562 UUIDv7: 48-bit ms timestamp, version 7, RFC variant. */
function uuidv7() {
  const bytes = randomBytes(16);
  const now = Date.now();
  bytes[0] = (now / 2 ** 40) & 0xff;
  bytes[1] = (now / 2 ** 32) & 0xff;
  bytes[2] = (now / 2 ** 24) & 0xff;
  bytes[3] = (now / 2 ** 16) & 0xff;
  bytes[4] = (now / 2 ** 8) & 0xff;
  bytes[5] = now & 0xff;
  bytes[6] = 0x70 | (bytes[6] & 0x0f); // version 7
  bytes[8] = 0x80 | (bytes[8] & 0x3f); // RFC 4122 variant
  const hex = [...bytes].map((b) => b.toString(16).padStart(2, '0')).join('');
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

/** Canonical work root bound to the reset: `dir-config.json` wins, else data dir. */
function resolveWorkDir(dataDir) {
  try {
    const raw = readFileSync(join(dataDir, DIR_CONFIG_FILE), 'utf8')
      .replace(/^\uFEFF/, ''); // tolerate a BOM from manual edits
    const parsed = JSON.parse(raw);
    if (typeof parsed.work_dir === 'string' && parsed.work_dir.length > 0) {
      return parsed.work_dir;
    }
  } catch {
    // Missing or unparsable dir-config: fall back to the data dir itself.
  }
  return dataDir;
}

async function confirm(promptText) {
  const rl = createInterface({ input: process.stdin, output: process.stdout });
  const answer = await new Promise((resolve) =>
    rl.question(`${promptText} (y/N) `, resolve),
  );
  rl.close();
  return answer.trim().toLowerCase() === 'y';
}

const args = process.argv.slice(2);
const dataDirArg = args.indexOf('--data-dir');
const dataDir =
  dataDirArg !== -1 && args[dataDirArg + 1]
    ? args[dataDirArg + 1]
    : devDataDir();
const assumeYes = args.includes('--yes');

if (!existsSync(dataDir)) {
  console.error(`✗ data dir does not exist: ${dataDir}`);
  console.error('  Launch the dev app once to create it, then re-run this script.');
  process.exit(1);
}

const databasePresent = DATABASE_LEAVES.some((leaf) =>
  existsSync(join(dataDir, leaf)),
);
if (!databasePresent) {
  console.error(`✗ no product database found under ${dataDir}`);
  console.error('  Nothing to reset — the dataset is already absent.');
  process.exit(1);
}

const workDir = resolveWorkDir(dataDir);
const requestPath = join(dataDir, RESET_REQUEST_FILE);

if (existsSync(requestPath)) {
  const existing = JSON.parse(readFileSync(requestPath, 'utf8'));
  if (
    existing.version === 2 &&
    existing.origin === 'user_explicit_factory_reset' &&
    existing.work_dir === workDir
  ) {
    console.log(`✓ an equivalent factory-reset request is already pending at`);
    console.log(`  ${requestPath}`);
    console.log('  Just restart the app to consume it.');
    process.exit(0);
  }
  console.error(`✗ a different reset request is already pending at ${requestPath}`);
  console.error('  Remove it manually once you understand why it differs, then re-run.');
  process.exit(1);
}

const request = {
  version: 2,
  operation_id: uuidv7(),
  requested_at: Date.now(),
  origin: 'user_explicit_factory_reset',
  work_dir: workDir,
};

console.log('Dev data dir :', dataDir);
console.log('Bound work dir:', workDir);
console.log('Operation    :', request.operation_id);
console.log('');
console.log(
  'This arms an explicit v3 factory reset. On the next app boot the current',
);
console.log(
  'dataset (database + side stores) is quarantined under retired-datasets/',
);
console.log('and a fresh empty v3 dataset is initialized. Nothing is deleted.');
console.log('');
console.log('Quit the app before restarting, or the running instance ignores it.');

if (!assumeYes && !(await confirm('Arm the factory reset?'))) {
  console.log('Aborted — no changes made.');
  process.exit(1);
}

mkdirSync(dirname(requestPath), { recursive: true });
writeFileSync(
  requestPath,
  `${JSON.stringify(request, null, 2)}\n`,
  { encoding: 'utf8', flag: 'wx' },
);
console.log(`✓ wrote ${requestPath}`);
console.log('  Restart the dev app; startup will retire the old dataset and begin fresh.');
