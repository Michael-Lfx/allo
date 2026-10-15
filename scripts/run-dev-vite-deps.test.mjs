import { describe, expect, test } from 'bun:test';
import { join } from 'node:path';

import {
  inspectViteOptimizeDeps,
  listedViteOptimizeDepFiles,
  reconcileViteOptimizeDeps,
  viteOptimizeDepDiskNames,
  viteOptimizeDepsDir,
  viteOptimizeDepsIsIntact,
} from './run-dev-vite-deps.mjs';

const metadata = {
  browserHash: '53206d8a',
  optimized: {
    react: { file: 'react.js' },
    three: { file: 'three.js' },
  },
  chunks: {
    'chunk-LIAUWEBJ': { file: 'chunk-LIAUWEBJ.js' },
    'chunk-5J4EPPAK': { file: 'chunk-5J4EPPAK.js' },
  },
};

describe('listedViteOptimizeDepFiles', () => {
  test('collects optimized entries and shared chunks', () => {
    expect(listedViteOptimizeDepFiles(metadata).sort()).toEqual([
      'chunk-5J4EPPAK.js',
      'chunk-LIAUWEBJ.js',
      'react.js',
      'three.js',
    ]);
  });

  test('ignores missing or malformed groups', () => {
    expect(listedViteOptimizeDepFiles({})).toEqual([]);
    expect(listedViteOptimizeDepFiles({ optimized: null, chunks: [] })).toEqual([]);
  });
});

describe('viteOptimizeDepDiskNames', () => {
  test('maps metadata __ encodings to on-disk dotted names', () => {
    expect(viteOptimizeDepDiskNames('subset-worker__chunk-PPE5HQR3.js')).toEqual([
      'subset-worker__chunk-PPE5HQR3.js',
      'subset-worker.chunk-PPE5HQR3.js',
    ]);
    expect(viteOptimizeDepDiskNames('react.js')).toEqual(['react.js']);
  });
});

describe('inspectViteOptimizeDeps', () => {
  test('reports a cold cache when metadata is absent', () => {
    expect(
      inspectViteOptimizeDeps('/deps', {
        exists: () => false,
        readFile: () => {
          throw new Error('should not read');
        },
      }),
    ).toEqual({ status: 'missing' });
  });

  test('reports incomplete when a shared chunk file is gone', () => {
    const inspection = inspectViteOptimizeDeps('/deps', {
      exists: (path) => path === join('/deps', '_metadata.json') || path.endsWith('react.js') || path.endsWith('three.js'),
      readFile: () => JSON.stringify(metadata),
    });
    expect(inspection.status).toBe('incomplete');
    expect(inspection.missing.sort()).toEqual(['chunk-5J4EPPAK.js', 'chunk-LIAUWEBJ.js']);
  });

  test('reports intact when every listed file exists', () => {
    const inspection = inspectViteOptimizeDeps('/deps', {
      exists: () => true,
      readFile: () => JSON.stringify(metadata),
    });
    expect(inspection.status).toBe('intact');
    expect(inspection.files).toHaveLength(4);
  });

  test('treats Vite __-encoded chunk names as present when the dotted file exists', () => {
    const encoded = {
      optimized: {},
      chunks: {
        'subset-worker.chunk-PPE5HQR3': { file: 'subset-worker__chunk-PPE5HQR3.js' },
        'image-blob-reduce.esm-5WX36VIY': { file: 'image-blob-reduce__esm-5WX36VIY.js' },
      },
    };
    const inspection = inspectViteOptimizeDeps('/deps', {
      exists: (path) =>
        path.endsWith('_metadata.json')
        || path.endsWith('subset-worker.chunk-PPE5HQR3.js')
        || path.endsWith('image-blob-reduce.esm-5WX36VIY.js'),
      readFile: () => JSON.stringify(encoded),
    });
    expect(inspection.status).toBe('intact');
  });
});

describe('reconcileViteOptimizeDeps', () => {
  test('does not wipe a consistent cache', () => {
    const removed = [];
    const result = reconcileViteOptimizeDeps('/ui', {
      exists: () => true,
      readFile: () => JSON.stringify(metadata),
      remove: (path) => removed.push(path),
      log: () => {},
    });
    expect(result.action).toBe('intact');
    expect(removed).toEqual([]);
    expect(viteOptimizeDepsIsIntact('/ui', { exists: () => true, readFile: () => JSON.stringify(metadata) })).toBe(true);
  });

  test('wipes an incomplete cache so Vite can rebuild it', () => {
    const removed = [];
    const logs = [];
    const result = reconcileViteOptimizeDeps('/ui', {
      exists: (path) => path.endsWith('_metadata.json'),
      readFile: () => JSON.stringify(metadata),
      remove: (path, opts) => removed.push({ path, opts }),
      log: (message) => logs.push(message),
    });
    expect(result.action).toBe('reset');
    expect(result.status).toBe('incomplete');
    expect(removed).toEqual([
      { path: viteOptimizeDepsDir('/ui'), opts: { recursive: true, force: true } },
    ]);
    expect(logs[0]).toContain('incomplete');
  });
});
