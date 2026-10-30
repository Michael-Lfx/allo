import { expect, test } from 'bun:test';
import fs from 'fs';
import path from 'path';
import crypto from 'crypto';
import {
  resolveCargoBinary,
  resolveBunBinary,
  verifyTraceArtifact,
  executeJointTraceRun,
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
  // Pass an invalid cargo binary to force step 1 failure
  const fakeCargo = path.join(repoRoot, 'non-existent-cargo-bin.exe');
  const result = executeJointTraceRun({
    cargoBin: fakeCargo,
    silent: true,
  });

  expect(result.exitCode).not.toBe(0);
  expect(result.tempDirCleaned).toBe(true);
  expect(result.error).toBeTruthy();
});
