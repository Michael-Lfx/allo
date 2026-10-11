import { describe, expect, test } from 'bun:test';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import catalog from './catalog.json';
import type { ModelStoreCatalogEntry } from './types';

const entries = catalog as ModelStoreCatalogEntry[];

const SUPPORTED_IDS = ['Qwen3.8-27B', 'Qwen3.6-35B-A3B', 'Qwen3.6-35B-A3B-MTP'];

describe('model store catalog', () => {
  test('contains exactly the three supported local models', () => {
    expect(entries.map((entry) => entry.id)).toEqual(SUPPORTED_IDS);
  });

  test('carries the directory metadata the store renders', () => {
    for (const entry of entries) {
      expect(entry.id.length).toBeGreaterThan(0);
      expect(entry.displayName.zh.length).toBeGreaterThan(0);
      expect(entry.displayName.en.length).toBeGreaterThan(0);
      expect(entry.type.length).toBeGreaterThan(0);
      expect(entry.sizeBytes).toBeGreaterThan(0);
    }
  });

  test('Qwen models use the Qwen mark', () => {
    const qwen = entries.filter((entry) => /^qwen/i.test(entry.id));
    expect(qwen.length).toBe(3);
    for (const entry of qwen) {
      expect(entry.icon).toBe('/model_logo/qwen.png');
    }
  });

  test('every referenced vendor logo is bundled into public/model_logo', () => {
    const icons = entries.filter((entry) => entry.icon);
    expect(icons.length).toBeGreaterThan(0);
    for (const entry of icons) {
      const file = fileURLToPath(new URL(`../../../../public${entry.icon}`, import.meta.url));
      expect(existsSync(file)).toBe(true);
    }
  });
});
