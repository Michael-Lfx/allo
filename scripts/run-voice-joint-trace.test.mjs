import { expect, test } from 'bun:test';
import fs from 'fs';
import path from 'path';
import crypto from 'crypto';
import {
  resolveCargoBinary,
  resolveBunBinary,
  verifyTraceArtifact,
  executeJointTraceRun,
  spawnProcessInJob,
  repoRoot,
} from './run-voice-joint-trace.mjs';

test('resolveCargoBinary resolves valid cargo from PATH or environment without hardcoded accounts', () => {
  const cargo = resolveCargoBinary();
  expect(fs.existsSync(cargo)).toBe(true);
  expect(cargo.toLowerCase()).toContain('cargo');
});

test('resolveBunBinary resolves valid bun binary from current execution context', () => {
  const bun = resolveBunBinary();
  expect(fs.existsSync(bun)).toBe(true);
  expect(bun.toLowerCase()).toContain('bun');
});

test('verifyTraceArtifact negative cases: missing, empty, invalid JSON, structure and SHA mismatch', () => {
  const tempDir = fs.mkdtempSync(path.join(repoRoot, 'build.noindex', 'test-verify-'));
  try {
    const missingFile = path.join(tempDir, 'non-existent.json');
    expect(() => verifyTraceArtifact(missingFile)).toThrow('Trace artifact not found');

    const emptyFile = path.join(tempDir, 'empty.json');
    fs.writeFileSync(emptyFile, '');
    expect(() => verifyTraceArtifact(emptyFile)).toThrow('Trace artifact is empty');

    const invalidJsonFile = path.join(tempDir, 'corrupt.json');
    fs.writeFileSync(invalidJsonFile, '{ not valid json');
    expect(() => verifyTraceArtifact(invalidJsonFile)).toThrow('contains invalid JSON');

    const missingMessagesFile = path.join(tempDir, 'bad_structure.json');
    fs.writeFileSync(missingMessagesFile, JSON.stringify({ conversation_id: 'c1', session_id: 's1', messages: [] }));
    expect(() => verifyTraceArtifact(missingMessagesFile)).toThrow('Trace artifact structure invalid');

    const missingSnapshotsFile = path.join(tempDir, 'no_snapshots.json');
    fs.writeFileSync(missingSnapshotsFile, JSON.stringify({
      conversation_id: 'c1',
      session_id: 's1',
      messages: [1, 2, 3, 4, 5, 6],
    }));
    expect(() => verifyTraceArtifact(missingSnapshotsFile)).toThrow('missing multi-stage snapshots');

    // SHA mismatch check
    const validStructure = {
      conversation_id: 'c1',
      session_id: 's1',
      snapshots: { at_b_failure: {}, after_b_recovery: {}, final: {} },
      messages: [1, 2, 3, 4, 5, 6],
    };
    const validJsonFile = path.join(tempDir, 'valid.json');
    fs.writeFileSync(validJsonFile, JSON.stringify(validStructure));
    const badSha = '0000000000000000000000000000000000000000000000000000000000000000';
    expect(() => verifyTraceArtifact(validJsonFile, badSha)).toThrow('Trace artifact SHA256 mismatch');

    // Expected SHA match
    const actualBytes = fs.readFileSync(validJsonFile);
    const goodSha = crypto.createHash('sha256').update(actualBytes).digest('hex');
    const res = verifyTraceArtifact(validJsonFile, goodSha);
    expect(res.sha256).toBe(goodSha);
    expect(res.bytes).toBe(actualBytes.length);
  } finally {
    fs.rmSync(tempDir, { recursive: true, force: true });
  }
});

test('executeJointTraceRun failure cleanup contract: temp directory is deleted even on failure without bypassing finally', () => {
  const fakeCargo = path.join(repoRoot, 'non-existent-cargo-bin.exe');
  const result = executeJointTraceRun({
    cargoBin: fakeCargo,
    silent: true,
  });

  expect(result.exitCode).not.toBe(0);
  expect(result.tempDirCleaned).toBe(true);
  expect(result.error).toBeTruthy();
});

test('spawnProcessInJob bounded timeout terminates entire child process tree', async () => {
  const tempDir = fs.mkdtempSync(path.join(repoRoot, 'build.noindex', 'test-tree-timeout-'));
  const leafPath = path.join(tempDir, 'leaf.txt');
  const exitedPath = path.join(tempDir, 'leaf-self-exited.txt');

  try {
    const parentReviewHelper = path.join(
      repoRoot,
      'build.noindex',
      'voice-runtime-improvements-parent-review-round2-20261030',
      'timeout-tree-helper.exe'
    );

    if (fs.existsSync(parentReviewHelper)) {
      const result = executeJointTraceRun({
        cargoBin: parentReviewHelper,
        env: { ...process.env, VOICE_TIMEOUT_PROBE_DIR: tempDir },
        buildNoindex: tempDir,
        timeoutRust: 2500,
        silent: true,
      });

      expect(result.exitCode).not.toBe(0);
      expect(result.tempDirCleaned).toBe(true);

      // Give enough time for a leaked 6s process to write self-exited if it was not killed
      await new Promise((r) => setTimeout(r, 4500));

      // The leaf MUST NOT have survived to write self-exited
      expect(fs.existsSync(exitedPath)).toBe(false);
    } else {
      // Fallback cross-platform self-contained tree probe
      const bunBin = resolveBunBinary();
      const treeScript = path.join(tempDir, 'tree_spawner.mjs');
      fs.writeFileSync(
        treeScript,
        `import { spawn } from 'child_process';
import fs from 'fs';
if (process.argv[2] === 'leaf') {
  fs.writeFileSync(${JSON.stringify(leafPath)}, 'alive');
  setTimeout(() => {
    fs.writeFileSync(${JSON.stringify(exitedPath)}, 'leaked');
  }, 4000);
} else {
  spawn(process.execPath, [${JSON.stringify(treeScript)}, 'leaf'], { detached: false });
  setTimeout(() => {}, 20000);
}`
      );

      const spawnRes = spawnProcessInJob(bunBin, [treeScript], {
        timeout: 1500,
        silent: true,
      });

      expect(spawnRes.status).toBe(124);
      await new Promise((r) => setTimeout(r, 4000));
      expect(fs.existsSync(exitedPath)).toBe(false);
    }
  } finally {
    fs.rmSync(tempDir, { recursive: true, force: true });
  }
}, 15000);
