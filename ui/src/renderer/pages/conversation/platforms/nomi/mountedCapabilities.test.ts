import { describe, expect, test } from 'bun:test';

import {
  getMountedCapabilities,
  hasMountedCapabilities,
  resolveMountedSkillLabel,
} from './mountedCapabilities';

describe('getMountedCapabilities', () => {
  test('reads skills and mcp_statuses from extra', () => {
    expect(
      getMountedCapabilities({
        extra: {
          skills: ['pdf', 'web-search'],
          mcp_statuses: [
            { mcp_server_id: 'srv-a', name: 'filesystem', status: 'loaded' },
            { mcp_server_id: 'srv-b', name: 'github', status: 'failed', reason: 'timeout' },
          ],
        },
      })
    ).toEqual({
      skills: ['pdf', 'web-search'],
      mcp: [
        { id: 'srv-a', name: 'filesystem', status: 'loaded' },
        { id: 'srv-b', name: 'github', status: 'failed', reason: 'timeout' },
      ],
    });
  });

  test('falls back to mcp_servers names when statuses are missing', () => {
    expect(
      getMountedCapabilities({
        extra: { mcp_servers: ['filesystem', 'github'] },
      }).mcp
    ).toEqual([
      { id: 'filesystem', name: 'filesystem' },
      { id: 'github', name: 'github' },
    ]);
  });

  test('prefers mcp_statuses over mcp_servers names', () => {
    expect(
      getMountedCapabilities({
        extra: {
          mcp_servers: ['legacy-name'],
          mcp_statuses: [{ mcp_server_id: 'srv-a', name: 'filesystem', status: 'unsupported' }],
        },
      }).mcp
    ).toEqual([{ id: 'srv-a', name: 'filesystem', status: 'unsupported' }]);
  });

  test('returns empty lists when extra has no mounts', () => {
    expect(getMountedCapabilities({ extra: {} })).toEqual({ skills: [], mcp: [] });
    expect(getMountedCapabilities({})).toEqual({ skills: [], mcp: [] });
  });
});

describe('hasMountedCapabilities', () => {
  test('is true when skills or mcp chips exist', () => {
    expect(hasMountedCapabilities({ skills: ['pdf'], mcp: [] })).toBe(true);
    expect(hasMountedCapabilities({ skills: [], mcp: [{ id: 'a', name: 'filesystem' }] })).toBe(true);
    expect(hasMountedCapabilities({ skills: [], mcp: [] })).toBe(false);
  });
});

describe('resolveMountedSkillLabel', () => {
  const catalog = [{ skillId: 'pdf-reader', name: 'PDF Reader', description: 'Read PDFs' }];

  test('uses catalog name when skillId matches', () => {
    expect(resolveMountedSkillLabel('pdf-reader', catalog)).toEqual({
      label: 'PDF Reader',
      description: 'Read PDFs',
    });
  });

  test('matches catalog by name', () => {
    expect(resolveMountedSkillLabel('PDF Reader', catalog)).toEqual({
      label: 'PDF Reader',
      description: 'Read PDFs',
    });
  });

  test('falls back to the snapshot id', () => {
    expect(resolveMountedSkillLabel('unknown-skill', [])).toEqual({ label: 'unknown-skill' });
  });
});
