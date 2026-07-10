#!/usr/bin/env bun
/**
 * Cross-platform launcher for upload-modelscope-release.py.
 *
 * Windows installs Python without always putting `pip` on PATH, and terminals
 * opened before install won't see `python` until restarted. This script resolves
 * a Python executable and invokes the upload script via `python -m`-style argv.
 */
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(fileURLToPath(new URL('.', import.meta.url)), '..');
const script = join(root, 'scripts', 'upload-modelscope-release.py');
const passthrough = process.argv.slice(2);

function probe(cmd, args) {
  const r = spawnSync(cmd, args, { encoding: 'utf8', stdio: 'pipe' });
  return !r.error && r.status === 0;
}

function resolvePython() {
  const candidates = ['python', 'python3', 'py'];
  for (const cmd of candidates) {
    if (probe(cmd, ['--version'])) return [cmd];
    if (cmd === 'py' && probe('py', ['-3', '--version'])) return ['py', '-3'];
  }

  if (process.platform === 'win32') {
    const localAppData = process.env.LOCALAPPDATA;
    if (localAppData) {
      for (const ver of ['Python312', 'Python313', 'Python311']) {
        const exe = join(localAppData, 'Programs', 'Python', ver, 'python.exe');
        if (existsSync(exe)) return [exe];
      }
    }
  }

  return null;
}

const python = resolvePython();
if (!python) {
  console.error('未找到 Python。请安装 Python 3 并确保 `python --version` 可用，然后重试。');
  console.error('Windows: winget install --id Python.Python.3.12 -e --source winget');
  console.error('安装后请新开终端，或执行:');
  console.error(
    '  $env:Path = [System.Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path","User")',
  );
  process.exit(1);
}

const result = spawnSync(python[0], [...python.slice(1), script, ...passthrough], {
  cwd: root,
  stdio: 'inherit',
  env: process.env,
});

process.exit(result.status ?? 1);
