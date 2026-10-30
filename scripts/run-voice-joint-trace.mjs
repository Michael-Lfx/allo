#!/usr/bin/env node
import { spawnSync } from 'child_process';
import fs from 'fs';
import path from 'path';
import crypto from 'crypto';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const repoRoot = path.resolve(__dirname, '..');

const buildNoindex = path.join(repoRoot, 'build.noindex');
if (!fs.existsSync(buildNoindex)) {
  fs.mkdirSync(buildNoindex, { recursive: true });
}

const tempDir = fs.mkdtempSync(path.join(buildNoindex, 'joint-trace-run-'));
const traceOutPath = path.join(tempDir, 'joint-trace.json');

console.log(`[INFO] Isolated trace output directory: ${tempDir}`);
console.log(`[INFO] Target trace path: ${traceOutPath}`);

try {
  // 1. Run Rust joint integration test
  console.log('[STEP 1] Running Rust joint test: test_voice_companion_joint_worker_to_host_three_sentence_chain...');
  const rustToolchainBin = 'C:\\Users\\38788\\.rustup\\toolchains\\1.97.1-x86_64-pc-windows-msvc\\bin';
  const env = {
    ...process.env,
    PATH: `${rustToolchainBin};${process.env.PATH || ''}`,
    VOICE_JOINT_TRACE_OUT_PATH: traceOutPath,
    CARGO_BUILD_JOBS: '2',
  };

  const rustResult = spawnSync(
    path.join(rustToolchainBin, 'cargo.exe'),
    ['test', '--locked', '-p', 'nomifun-app', '--test', 'voice_dialogue_integration', 'test_voice_companion_joint', '--', '--nocapture'],
    { cwd: repoRoot, env, stdio: 'inherit' }
  );

  if (rustResult.status !== 0) {
    console.error(`[FAIL] Rust joint test failed with exit code: ${rustResult.status}`);
    process.exit(rustResult.status || 1);
  }

  // 2. Validate trace artifact
  console.log('[STEP 2] Verifying trace artifact...');
  if (!fs.existsSync(traceOutPath)) {
    console.error(`[FAIL] Trace artifact was not created at ${traceOutPath}`);
    process.exit(1);
  }

  const rawBytes = fs.readFileSync(traceOutPath);
  if (rawBytes.length === 0) {
    console.error(`[FAIL] Trace artifact is empty: 0 bytes`);
    process.exit(1);
  }

  const sha256 = crypto.createHash('sha256').update(rawBytes).digest('hex');
  console.log(`[INFO] Trace SHA256: ${sha256} (${rawBytes.length} bytes)`);

  let parsed;
  try {
    parsed = JSON.parse(rawBytes.toString('utf8'));
  } catch (err) {
    console.error(`[FAIL] Trace artifact contains invalid JSON: ${err}`);
    process.exit(1);
  }

  if (!parsed.conversation_id || !parsed.session_id || !Array.isArray(parsed.messages) || parsed.messages.length !== 6) {
    console.error(`[FAIL] Trace artifact structure does not conform to specification (messages length !== 6)`);
    process.exit(1);
  }

  // 3. Run Bun UI integration test consuming the exact trace artifact
  console.log('[STEP 3] Running Bun UI test: jointTraceReplay.test.ts with VOICE_JOINT_TRACE_PATH...');
  const bunPath = 'C:\\Users\\38788\\.bun\\bin\\bun.exe';
  const bunEnv = {
    ...process.env,
    VOICE_JOINT_TRACE_PATH: traceOutPath,
  };

  const bunResult = spawnSync(
    bunPath,
    ['test', 'src/renderer/pages/voiceSession/jointTraceReplay.test.ts'],
    { cwd: path.join(repoRoot, 'ui'), env: bunEnv, stdio: 'inherit' }
  );

  if (bunResult.status !== 0) {
    console.error(`[FAIL] Bun UI test failed with exit code: ${bunResult.status}`);
    process.exit(bunResult.status || 1);
  }

  console.log(`[SUCCESS] Rust -> Bun end-to-end joint trace integration verified cleanly!`);
} finally {
  try {
    fs.rmSync(tempDir, { recursive: true, force: true });
    console.log(`[INFO] Cleaned up temporary directory: ${tempDir}`);
  } catch {}
}
