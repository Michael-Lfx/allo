import { expect, test } from 'bun:test';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');

test('offline preparation consumes the shared manifest and preserves transactional safety', () => {
  const result = Bun.spawnSync(
    [process.env.PYTHON ?? 'python', join(ROOT, 'scripts', 'prepare_voice_models_test.py'), '-v'],
    { cwd: ROOT, stdout: 'pipe', stderr: 'pipe' },
  );
  const output = Buffer.concat([result.stdout, result.stderr]).toString();
  expect(output).toContain('test_shared_manifest_complete_and_pinned');
  expect(output).toContain('test_data_roots_and_override');
  expect(result.exitCode, output).toBe(0);
}, 30_000);
