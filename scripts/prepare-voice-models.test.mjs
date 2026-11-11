import { expect, test } from 'bun:test';
import { readFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');

test('offline downloader manifest covers every pinned TTS asset', async () => {
  const python = await readFile(join(ROOT, 'scripts', 'prepare-voice-models.py'), 'utf8');
  const rust = await readFile(
    join(ROOT, 'crates', 'backend', 'nomifun-voice-models', 'src', 'verifier.rs'),
    'utf8',
  );
  const manifestBlock = python.match(/MODELS_MANIFEST = \[([\s\S]*?)\n\]/)?.[1] ?? '';
  const pythonEntries = new Map();
  for (const block of manifestBlock.split('\n    {').slice(1)) {
    const rel = block.match(/"rel_path": "([^"]+)"/)?.[1];
    const sha = block.match(/"sha256":(?: "([0-9a-f]+)")?/)?.[1];
    if (rel) pythonEntries.set(rel, sha);
  }
  const rustTts = new Map(
    [...rust.matchAll(/manifest\.insert\("(tts\/[^"]+)"\.to_string\(\), "([0-9a-f]+)"\.to_string\(\)\);/g)].map(
      (match) => [match[1], match[2]],
    ),
  );
  expect(rustTts.size).toBe(16);
  for (const [relative, sha] of rustTts) {
    expect(pythonEntries.get(relative)).toBe(sha);
  }
  expect([...pythonEntries.keys()].filter((path) => path.startsWith('tts/'))).toHaveLength(16);
});

test('offline default targets the desktop app data root instead of the legacy folder', async () => {
  const python = await readFile(join(ROOT, 'scripts', 'prepare-voice-models.py'), 'utf8');
  expect(python).toContain('Path(local_app_data) / "Flowy" / leaf / "models"');
  expect(python).not.toContain('"Flowy" / "Voice-Companion" / "models"');
  expect(python).toContain('channel == "stable" else f"Nomi-{channel}"');
  expect(python).toContain('def detect_default_channel()');
  expect(python).toContain('repo_root / "apps" / "desktop" / "src" / "main.rs"');
  expect(python).toContain('"--channel"');
  expect(python).toContain('hf_official_url');
});
