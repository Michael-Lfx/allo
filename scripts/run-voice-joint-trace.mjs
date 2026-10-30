#!/usr/bin/env node
import { spawnSync } from 'child_process';
import fs from 'fs';
import path from 'path';
import crypto from 'crypto';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
export const repoRoot = path.resolve(__dirname, '..');

export function resolveCargoBinary(env = process.env) {
  if (env.CARGO_BIN && fs.existsSync(env.CARGO_BIN)) {
    return env.CARGO_BIN;
  }
  if (env.CARGO && fs.existsSync(env.CARGO)) {
    return env.CARGO;
  }
  // Check system PATH
  const pathExt = process.platform === 'win32' ? ['.exe', '.cmd', ''] : [''];
  const pathDirs = (env.PATH || '').split(path.delimiter);
  for (const dir of pathDirs) {
    if (!dir) continue;
    for (const ext of pathExt) {
      const candidate = path.join(dir, `cargo${ext}`);
      if (fs.existsSync(candidate)) {
        return candidate;
      }
    }
  }
  // On Windows, also inspect standard rustup location in USERPROFILE
  if (process.platform === 'win32' && env.USERPROFILE) {
    const rustupBin = path.join(env.USERPROFILE, '.cargo', 'bin', 'cargo.exe');
    if (fs.existsSync(rustupBin)) {
      return rustupBin;
    }
    const toolchainsDir = path.join(env.USERPROFILE, '.rustup', 'toolchains');
    if (fs.existsSync(toolchainsDir)) {
      try {
        const toolchains = fs.readdirSync(toolchainsDir);
        for (const tc of toolchains) {
          const candidate = path.join(toolchainsDir, tc, 'bin', 'cargo.exe');
          if (fs.existsSync(candidate)) {
            return candidate;
          }
        }
      } catch {}
    }
  }
  throw new Error('cargo executable could not be resolved from environment or PATH');
}

export function resolveBunBinary(env = process.env) {
  if (env.BUN_BIN && fs.existsSync(env.BUN_BIN)) {
    return env.BUN_BIN;
  }
  if (process.execPath && fs.existsSync(process.execPath) && process.execPath.toLowerCase().includes('bun')) {
    return process.execPath;
  }
  // Check system PATH
  const pathExt = process.platform === 'win32' ? ['.exe', '.cmd', ''] : [''];
  const pathDirs = (env.PATH || '').split(path.delimiter);
  for (const dir of pathDirs) {
    if (!dir) continue;
    for (const ext of pathExt) {
      const candidate = path.join(dir, `bun${ext}`);
      if (fs.existsSync(candidate)) {
        return candidate;
      }
    }
  }
  return process.execPath; // Fallback to current runtime
}

export function verifyTraceArtifact(tracePath, expectedSha = null) {
  if (!fs.existsSync(tracePath)) {
    throw new Error(`Trace artifact not found at ${tracePath}`);
  }
  const rawBytes = fs.readFileSync(tracePath);
  if (rawBytes.length === 0) {
    throw new Error(`Trace artifact is empty: 0 bytes`);
  }
  const actualSha = crypto.createHash('sha256').update(rawBytes).digest('hex');
  if (expectedSha && actualSha.toLowerCase() !== expectedSha.toLowerCase()) {
    throw new Error(`Trace artifact SHA256 mismatch: expected ${expectedSha}, got ${actualSha}`);
  }
  let parsed;
  try {
    parsed = JSON.parse(rawBytes.toString('utf8'));
  } catch (err) {
    throw new Error(`Trace artifact contains invalid JSON: ${err}`);
  }
  if (!parsed.conversation_id || !parsed.session_id || !Array.isArray(parsed.messages) || parsed.messages.length !== 6) {
    throw new Error('Trace artifact structure invalid: conversation_id, session_id or 6 messages missing');
  }
  if (!parsed.snapshots || !parsed.snapshots.at_b_failure || !parsed.snapshots.after_b_recovery || !parsed.snapshots.final) {
    throw new Error('Trace artifact missing multi-stage snapshots: at_b_failure, after_b_recovery, final required');
  }
  return { sha256: actualSha, bytes: rawBytes.length, parsed };
}

export function executeJointTraceRun(options = {}) {
  const buildNoindex = options.buildNoindex || path.join(repoRoot, 'build.noindex');
  if (!fs.existsSync(buildNoindex)) {
    fs.mkdirSync(buildNoindex, { recursive: true });
  }

  const tempDir = fs.mkdtempSync(path.join(buildNoindex, 'joint-trace-run-'));
  const traceOutPath = path.join(tempDir, 'joint-trace.json');

  let exitCode = 0;
  let runError = null;

  try {
    const cargoBin = options.cargoBin || resolveCargoBinary(options.env || process.env);
    const bunBin = options.bunBin || resolveBunBinary(options.env || process.env);

    if (!options.silent) {
      console.log(`[INFO] Isolated trace output directory: ${tempDir}`);
      console.log(`[INFO] Cargo binary: ${cargoBin}`);
      console.log(`[INFO] Bun binary: ${bunBin}`);
    }

    // Step 1: Rust joint integration test
    if (!options.silent) {
      console.log('[STEP 1] Running Rust joint test: test_voice_companion_joint_worker_to_host_three_sentence_chain...');
    }
    const rustEnv = {
      ...(options.env || process.env),
      VOICE_JOINT_TRACE_OUT_PATH: traceOutPath,
      CARGO_BUILD_JOBS: '2',
    };

    const rustResult = spawnSync(
      cargoBin,
      ['test', '--locked', '-p', 'nomifun-app', '--test', 'voice_dialogue_integration', 'test_voice_companion_joint', '--', '--nocapture'],
      {
        cwd: repoRoot,
        env: rustEnv,
        stdio: options.silent ? 'pipe' : 'inherit',
        timeout: options.timeoutRust || 180000,
        killSignal: 'SIGKILL',
      }
    );

    if (rustResult.error) {
      throw new Error(`Rust test process error: ${rustResult.error.message}`);
    }
    if (rustResult.status !== 0) {
      exitCode = rustResult.status || 1;
      throw new Error(`Rust joint test exited with code ${rustResult.status}`);
    }

    // Step 2: Validate trace artifact & compute producer SHA256
    if (!options.silent) {
      console.log('[STEP 2] Verifying trace artifact & computing SHA256...');
    }
    const verified = verifyTraceArtifact(traceOutPath);
    if (!options.silent) {
      console.log(`[INFO] Verified Trace SHA256: ${verified.sha256} (${verified.bytes} bytes)`);
    }

    // Step 3: Run Bun UI integration test with cryptographic expected SHA
    if (!options.silent) {
      console.log('[STEP 3] Running Bun UI test with consumer SHA256 verification...');
    }
    const bunEnv = {
      ...(options.env || process.env),
      VOICE_JOINT_TRACE_PATH: traceOutPath,
      VOICE_JOINT_TRACE_EXPECTED_SHA: verified.sha256,
    };

    const bunResult = spawnSync(
      bunBin,
      ['test', 'src/renderer/pages/voiceSession/jointTraceReplay.test.ts'],
      {
        cwd: path.join(repoRoot, 'ui'),
        env: bunEnv,
        stdio: options.silent ? 'pipe' : 'inherit',
        timeout: options.timeoutBun || 45000,
        killSignal: 'SIGKILL',
      }
    );

    if (bunResult.error) {
      throw new Error(`Bun test process error: ${bunResult.error.message}`);
    }
    if (bunResult.status !== 0) {
      exitCode = bunResult.status || 1;
      throw new Error(`Bun UI test exited with code ${bunResult.status}`);
    }

    if (!options.silent) {
      console.log('[SUCCESS] Rust -> Bun end-to-end joint trace integration verified cleanly!');
    }
  } catch (err) {
    runError = err;
    if (exitCode === 0) exitCode = 1;
    if (!options.silent) {
      console.error(`[FAIL] ${err.message}`);
    }
  } finally {
    try {
      if (fs.existsSync(tempDir)) {
        fs.rmSync(tempDir, { recursive: true, force: true });
        if (!options.silent) {
          console.log(`[INFO] Cleaned up temporary directory: ${tempDir}`);
        }
      }
    } catch (cleanupErr) {
      console.error(`[ERROR] Failed to clean up ${tempDir}: ${cleanupErr}`);
      exitCode = exitCode || 1;
    }
  }

  return { exitCode, tempDirCleaned: !fs.existsSync(tempDir), error: runError };
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(__filename)) {
  const result = executeJointTraceRun();
  process.exit(result.exitCode);
}
