import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('slash launcher dismissal lifecycle', () => {
  test('clears a prior dismissal after the active slash token ends', () => {
    const source = readSource(new URL('./useSlashLauncherController.ts', import.meta.url));

    expect(source.includes('if (query === null) {\n      setDismissedQuery(null);\n    }')).toBe(true);
  });
});
