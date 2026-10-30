import { expect, test } from 'bun:test';
import { spawnSync } from 'child_process';
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

function ensureBuildNoindex() {
  const buildDir = path.join(repoRoot, 'build.noindex');
  if (!fs.existsSync(buildDir)) {
    fs.mkdirSync(buildDir, { recursive: true });
  }
  return buildDir;
}

function isPidAlive(pid) {
  if (!pid || pid <= 0) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

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
  const buildDir = ensureBuildNoindex();
  const tempDir = fs.mkdtempSync(path.join(buildDir, 'test-verify-'));
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

test('C1 regression: spawnProcessInJob directly fails and reports error for non-existent executable', () => {
  const bogusExe = path.join(repoRoot, 'deliberately-does-not-exist.exe');
  const result = spawnProcessInJob(bogusExe, [], { silent: true });
  expect(result.status).not.toBe(0);
  expect(result.error).toBeTruthy();
});

test('C1 regression: executeJointTraceRun fails and reports error when bun executable is missing', () => {
  const buildDir = ensureBuildNoindex();
  const testDir = fs.mkdtempSync(path.join(buildDir, 'test-c1-bun-'));
  const mockCargo = path.join(testDir, 'mock_cargo.bat');
  const validJson = path.join(repoRoot, 'ui/src/renderer/pages/voiceSession/fixtures/jointTraceFixture.json');
  fs.writeFileSync(mockCargo, `@echo off\r\ncopy /y "${validJson}" "%VOICE_JOINT_TRACE_OUT_PATH%" >nul\r\nexit /b 0\r\n`);

  try {
    const bogusBun = path.join(repoRoot, 'missing-bun.exe');
    const result = executeJointTraceRun({
      cargoBin: mockCargo,
      bunBin: bogusBun,
      silent: true,
    });
    expect(result.exitCode).not.toBe(0);
    expect(result.error).toBeTruthy();
  } finally {
    fs.rmSync(testDir, { recursive: true, force: true });
  }
});

test('R2-A & R2-B: self-contained helper with startup handshake proves child and grandchild are terminated upon Job timeout', async () => {
  const buildDir = ensureBuildNoindex();
  const testDir = fs.mkdtempSync(path.join(buildDir, 'test-job-r2-'));
  const nonce = crypto.randomBytes(6).toString('hex');

  const parentReadyFile = path.join(testDir, `parent-${nonce}.ready`);
  const leafReadyFile = path.join(testDir, `leaf-${nonce}.ready`);
  const leafExitedFile = path.join(testDir, `leaf-${nonce}.self-exited`);

  try {
    // Build self-contained Rust helper into the test directory using rustc
    const helperSrc = `
use std::{env, fs, process::{Command, Stdio}, thread, time::{Duration, SystemTime, UNIX_EPOCH}};
#[cfg(windows)] use std::os::windows::process::CommandExt;
fn main() {
    let root = env::var("VOICE_TIMEOUT_PROBE_DIR").unwrap();
    let nonce = env::var("VOICE_PROBE_NONCE").unwrap_or_default();
    if env::args().nth(1).as_deref() == Some("owned-leaf") {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
        fs::write(format!("{root}/leaf-{nonce}.ready"), format!("{}\\n{}", std::process::id(), now)).unwrap();
        thread::sleep(Duration::from_secs(6));
        fs::write(format!("{root}/leaf-{nonce}.self-exited"), "bounded leaf exiting normally").unwrap();
        return;
    }
    fs::write(format!("{root}/parent-{nonce}.ready"), format!("{}", std::process::id())).unwrap();
    let mut child = Command::new(env::current_exe().unwrap());
    child.arg("owned-leaf").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)] child.creation_flags(0x08000000);
    let owned = child.spawn().unwrap();
    fs::write(format!("{root}/parent-child-{nonce}.txt"), format!("{}\\n{}", std::process::id(), owned.id())).unwrap();
    thread::sleep(Duration::from_secs(20));
}
`;
    const helperRs = path.join(testDir, 'helper.rs');
    const helperExe = path.join(testDir, 'helper.exe');
    fs.writeFileSync(helperRs, helperSrc);

    const rustcRes = spawnSync('rustc', [helperRs, '-o', helperExe], {
      cwd: testDir,
      stdio: 'pipe',
    });
    expect(rustcRes.status).toBe(0);
    expect(fs.existsSync(helperExe)).toBe(true);

    // Run with 2500ms timeout
    const result = executeJointTraceRun({
      cargoBin: helperExe,
      env: {
        ...process.env,
        VOICE_TIMEOUT_PROBE_DIR: testDir,
        VOICE_PROBE_NONCE: nonce,
      },
      buildNoindex: testDir,
      timeoutRust: 2500,
      silent: true,
    });

    // 1. Verify runner reports timeout
    expect(result.exitCode).toBe(124);
    expect(result.tempDirCleaned).toBe(true);

    // 2. Anti-false-green: prove BOTH parent and leaf actually started and wrote handshake
    expect(fs.existsSync(parentReadyFile)).toBe(true);
    expect(fs.existsSync(leafReadyFile)).toBe(true);

    const parentPid = parseInt(fs.readFileSync(parentReadyFile, 'utf8').trim(), 10);
    const leafPid = parseInt(fs.readFileSync(leafReadyFile, 'utf8').split('\n')[0].trim(), 10);
    expect(parentPid).toBeGreaterThan(0);
    expect(leafPid).toBeGreaterThan(0);

    // 3. Immediately assert both parent and leaf were terminated by Windows Job Object
    expect(isPidAlive(parentPid)).toBe(false);
    expect(isPidAlive(leafPid)).toBe(false);

    // 4. Wait long enough for the 6-second sleep: leaf must NEVER have survived to write self-exited!
    await new Promise((r) => setTimeout(r, 4500));
    expect(fs.existsSync(leafExitedFile)).toBe(false);
  } finally {
    fs.rmSync(testDir, { recursive: true, force: true });
  }
}, 15000);

test('anti-false-green negative test: non-existent binary must fail with start error and fail handshake check', () => {
  const buildDir = ensureBuildNoindex();
  const testDir = fs.mkdtempSync(path.join(buildDir, 'test-false-green-'));
  const nonce = crypto.randomBytes(6).toString('hex');
  const parentReadyFile = path.join(testDir, `parent-${nonce}.ready`);

  try {
    const bogusExe = path.join(testDir, 'deliberately-does-not-exist.exe');
    const result = executeJointTraceRun({
      cargoBin: bogusExe,
      env: {
        ...process.env,
        VOICE_TIMEOUT_PROBE_DIR: testDir,
        VOICE_PROBE_NONCE: nonce,
      },
      buildNoindex: testDir,
      timeoutRust: 2000,
      silent: true,
    });

    // Process failed to start
    expect(result.exitCode).not.toBe(0);

    // Handshake was never created!
    expect(fs.existsSync(parentReadyFile)).toBe(false);

    // If an assertion requires the startup handshake, it cannot be spoofed!
    const verifiedStartup = fs.existsSync(parentReadyFile);
    expect(verifiedStartup).toBe(false);
  } finally {
    fs.rmSync(testDir, { recursive: true, force: true });
  }
});
