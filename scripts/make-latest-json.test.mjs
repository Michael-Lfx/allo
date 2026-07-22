#!/usr/bin/env bun
import assert from 'node:assert/strict';
import { rmSync, mkdirSync, writeFileSync, readFileSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const version = readWorkspaceVersion();
const testDir = join(root, 'build.noindex', 'make-latest-json-test');
const targetDir = join(testDir, 'target');
const outPath = join(testDir, 'latest.json');
const existingNotes = `Existing release notes for ${version}`;

rmSync(testDir, { recursive: true, force: true });
mkdirSync(testDir, { recursive: true });

const windowsFixtureDir = join(targetDir, 'x86_64-test-windows-msvc', 'release', 'bundle', 'nsis');
const windowsArtifact = join(windowsFixtureDir, `Flowy_${version}_x64-setup.exe`);
writeArtifactWithSig(windowsArtifact, 'fake windows installer', 'fake windows signature');

const linuxFixtureDir = join(targetDir, 'x86_64-unknown-linux-gnu', 'release', 'bundle');
const appImageArtifact = join(linuxFixtureDir, 'appimage', `Flowy_${version}_amd64.AppImage`);
const debArtifact = join(linuxFixtureDir, 'deb', `Flowy_${version}_amd64.deb`);
const rpmArtifact = join(linuxFixtureDir, 'rpm', `Flowy-${version}-1.x86_64.rpm`);
writeArtifactWithSig(rpmArtifact, 'fake rpm', 'fake rpm signature');
writeArtifactWithSig(debArtifact, 'fake deb', 'fake deb signature');
writeArtifactWithSig(appImageArtifact, 'fake appimage', 'fake appimage signature');

writeFileSync(
  outPath,
  JSON.stringify(
    {
      version,
      notes: existingNotes,
      pub_date: '2026-07-05T00:00:00.000Z',
      platforms: {
        'darwin-x86_64': {
          signature: 'existing darwin signature',
          url: `https://github.com/example/repo/releases/download/v${version}/Flowy.app.tar.gz`,
        },
      },
    },
    null,
    2,
  ) + '\n',
);

try {
  const scriptPath = join(root, 'scripts', 'make-latest-json.mjs');
  const result = spawnSync('bun', [scriptPath, '--out', outPath, '--repo', 'example/repo', '--target-dir', targetDir], {
    cwd: root,
    encoding: 'utf8',
  });

  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = JSON.parse(readFileSync(outPath, 'utf8'));
  assert.equal(manifest.version, version);
  assert.equal(manifest.notes, existingNotes);
  assert.ok(manifest.platforms['darwin-x86_64']);
  assert.ok(manifest.platforms['windows-x86_64']);
  assert.ok(manifest.platforms['linux-x86_64'], 'linux-x86_64 must be present');
  assert.equal(
    manifest.platforms['linux-x86_64'].url,
    `https://github.com/example/repo/releases/download/v${version}/Flowy_${version}_amd64.AppImage`,
  );
  assert.equal(manifest.platforms['linux-x86_64'].signature, 'fake appimage signature');
} finally {
  rmSync(testDir, { recursive: true, force: true });
}

// ── ModelScope URL generation + alpha.yml ───────────────────────────────────
{
  const msTestDir = join(root, 'build.noindex', 'make-latest-json-ms-test');
  const msTargetDir = join(msTestDir, 'target');
  const msOutPath = join(msTestDir, 'latest.json');
  const msDist = join(root, 'dist', 'desktop');
  const alphaPath = join(root, 'apps', 'desktop', 'updater', 'alpha.yml');
  const alphaBefore = existsSync(alphaPath) ? readFileSync(alphaPath, 'utf8') : null;

  rmSync(msTestDir, { recursive: true, force: true });
  mkdirSync(msTestDir, { recursive: true });

  const winDir = join(msTargetDir, 'x86_64-pc-windows-msvc', 'release', 'bundle', 'nsis');
  const winArtifact = join(winDir, `Flowy_${version}_x64-setup.exe`);
  writeArtifactWithSig(winArtifact, 'fake', 'sig');

  const linuxDir = join(msTargetDir, 'x86_64-unknown-linux-gnu', 'release', 'bundle', 'appimage');
  const linuxArtifact = join(linuxDir, `Flowy_${version}_x86_64.AppImage`);
  writeArtifactWithSig(linuxArtifact, 'fake linux', 'linux-sig');

  const scriptPath = join(root, 'scripts', 'make-latest-json.mjs');
  try {
    const msResult = spawnSync(
      'bun',
      [
        scriptPath,
        '--out',
        msOutPath,
        '--host',
        'modelscope',
        '--ms-repo',
        'flowy2025/flowyaipc',
        '--ms-prefix',
        'allo',
        '--channel',
        'alpha',
        '--collect',
        '--target-dir',
        msTargetDir,
      ],
      { cwd: root, encoding: 'utf8' },
    );
    assert.equal(msResult.status, 0, msResult.stderr || msResult.stdout);

    const msManifest = JSON.parse(readFileSync(msOutPath, 'utf8'));
    assert.equal(
      msManifest.platforms['windows-x86_64'].url,
      `https://modelscope.cn/api/v1/models/flowy2025/flowyaipc/repo?Revision=master&FilePath=allo/v${version}/Flowy_${version}_x64-setup.exe`,
    );
    assert.ok(msManifest.platforms['linux-x86_64']);
    assert.match(msManifest.platforms['linux-x86_64'].url, /Flowy_.*_x86_64\.AppImage/);

    const alpha = readFileSync(alphaPath, 'utf8');
    assert.match(alpha, new RegExp(`version: "${version}"`));
    assert.match(alpha, /channel: alpha/);
    assert.match(alpha, /manifest: channels\/alpha\/latest\.json/);
    assert.ok(existsSync(join(msDist, 'alpha.yml')));
    assert.ok(existsSync(join(msDist, 'latest.json')));
  } finally {
    rmSync(msTestDir, { recursive: true, force: true });
    if (alphaBefore !== null) writeFileSync(alphaPath, alphaBefore);
  }
}

// ── Reject legacy NomiFun_* names ───────────────────────────────────────────
{
  const legacyDir = join(root, 'build.noindex', 'make-latest-json-legacy-test');
  const legacyTarget = join(legacyDir, 'target');
  const legacyOut = join(legacyDir, 'latest.json');
  rmSync(legacyDir, { recursive: true, force: true });
  mkdirSync(legacyDir, { recursive: true });

  const winDir = join(legacyTarget, 'x86_64-pc-windows-msvc', 'release', 'bundle', 'nsis');
  writeArtifactWithSig(join(winDir, `NomiFun_${version}_x64-setup.exe`), 'legacy', 'sig');

  const scriptPath = join(root, 'scripts', 'make-latest-json.mjs');
  const result = spawnSync(
    'bun',
    [scriptPath, '--out', legacyOut, '--target-dir', legacyTarget],
    { cwd: root, encoding: 'utf8' },
  );
  assert.notEqual(result.status, 0, 'legacy NomiFun_* names must fail');
  assert.match(result.stderr || result.stdout, /NomiFun|遗留/);
  rmSync(legacyDir, { recursive: true, force: true });
}

function writeArtifactWithSig(path, artifactContent, signatureContent) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, artifactContent);
  writeFileSync(`${path}.sig`, signatureContent);
}

function readWorkspaceVersion() {
  const lines = readFileSync(join(root, 'Cargo.toml'), 'utf8').split('\n');
  let inSection = false;
  for (const line of lines) {
    const trimmed = line.trim();
    if (trimmed.startsWith('[')) {
      inSection = trimmed === '[workspace.package]';
      continue;
    }
    if (inSection) {
      const match = line.match(/^\s*version\s*=\s*"([^"]+)"/);
      if (match) return match[1];
    }
  }
  throw new Error('Unable to read workspace version');
}
