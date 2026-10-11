import { describe, expect, test } from 'bun:test';
import {
  HOUMO_PREFIX,
  assertHoumoPath,
  assertManifestHoumo,
  houmoArtifactRemotePath,
  houmoManifestRemotePath,
  houmoManifestViolations,
  resolveChannel,
} from './upload-houmo-ota.mjs';

describe('upload:houmo-ota paths', () => {
  test('manifest and artifact remote paths are locked to allo/houmo', () => {
    expect(HOUMO_PREFIX).toBe('allo/houmo');
    const manifestPath = houmoManifestRemotePath('windows');
    const artifactPath = houmoArtifactRemotePath('windows', '1.4.1', 'Flowy_1.4.1_x64-setup.exe');
    expect(manifestPath).toBe('allo/houmo/channels/windows/latest.json');
    expect(artifactPath).toBe('allo/houmo/windows/v1.4.1/Flowy_1.4.1_x64-setup.exe');
    expect(manifestPath).toContain('houmo');
    expect(artifactPath).toContain('houmo');
  });

  test('accepts houmo paths and rejects paths without houmo', () => {
    expect(assertHoumoPath('allo/houmo/channels/windows/latest.json')).toContain('houmo');
    expect(() => assertHoumoPath('allo/channels/windows/latest.json')).toThrow();
    expect(() => assertHoumoPath(undefined)).toThrow();
    expect(() => assertHoumoPath('')).toThrow();
  });

  test('flags manifest URLs that drift off the houmo prefix', () => {
    const base = 'https://modelscope.cn/api/v1/models/flowy2025/flowyaipc/repo?Revision=master&FilePath=';
    const ok = {
      version: '1.4.1',
      platforms: {
        'windows-x86_64': { url: `${base}allo/houmo/windows/v1.4.1/Flowy_1.4.1_x64-setup.exe` },
      },
    };
    expect(houmoManifestViolations(ok)).toEqual([]);
    expect(() => assertManifestHoumo(ok, 'windows')).not.toThrow();

    const bad = {
      version: '1.4.1',
      platforms: {
        'windows-x86_64': { url: `${base}allo/windows/v1.4.1/Flowy_1.4.1_x64-setup.exe` },
      },
    };
    expect(houmoManifestViolations(bad)).toHaveLength(1);
    expect(() => assertManifestHoumo(bad, 'windows')).toThrow(/houmo/);
  });

  test('resolveChannel validates the platform channel', () => {
    expect(resolveChannel('linux')).toBe('linux');
    expect(() => resolveChannel('alpha')).toThrow();
  });
});
