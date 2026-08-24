import { describe, expect, test } from 'bun:test';

import { getCompanionDisclosureIds, getWorkpathDisclosureIds } from './disclosureIds';

describe('disclosure ids', () => {
  test('keeps distinct workpaths and overflow regions addressable', () => {
    const first = getWorkpathDisclosureIds('D:/foo/项目一', ':r0:');
    const second = getWorkpathDisclosureIds('D:/foo/项目二', ':r0:');
    const punctuationVariant = getWorkpathDisclosureIds('D:/foo/a b', ':r0:');
    const underscoreVariant = getWorkpathDisclosureIds('D:/foo/a_b', ':r0:');

    expect(first.sessionsId).not.toBe(second.sessionsId);
    expect(punctuationVariant.sessionsId).not.toBe(underscoreVariant.sessionsId);
    expect(first.sessionsId).not.toContain(' ');
    expect(first.overflowId).not.toBe(first.sessionsId);
    expect(first.overflowId).toContain('-overflow');
  });

  test('namespaces separate mounted companion groups', () => {
    const first = getCompanionDisclosureIds(':r0:');
    const second = getCompanionDisclosureIds(':r1:');

    expect(first.sessionsId).not.toBe(second.sessionsId);
    expect(first.overflowId).not.toBe(first.sessionsId);
  });
});
