import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('./useWorkpathBranches.ts', import.meta.url), 'utf8');

describe('useWorkpathBranch performance contract', () => {
  test('loads only near-viewport workpaths and bounds shared lookup concurrency', () => {
    expect(source).toContain('IntersectionObserver');
    expect(source).toContain("rootMargin: '200px 0px'");
    expect(source).toContain('MAX_CONCURRENT_BRANCH_LOOKUPS = 4');
    expect(source).toContain('activeBranchLookups < MAX_CONCURRENT_BRANCH_LOOKUPS');
    expect(source).toContain('branchInFlight');
  });

  test('uses a short-lived cache and avoids eager Promise.all fan-out', () => {
    expect(source).toContain('BRANCH_CACHE_TTL_MS = 30_000');
    expect(source).toContain('expiresAt: Date.now() + BRANCH_CACHE_TTL_MS');
    expect(source).not.toContain('Promise.all(');
  });
});
