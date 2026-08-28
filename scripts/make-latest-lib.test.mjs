import { describe, expect, test } from 'bun:test';
import { platformKeysForArtifactName } from './make-latest-lib.mjs';

describe('platformKeysForArtifactName', () => {
  test('maps Windows setup exes to arch keys', () => {
    expect(platformKeysForArtifactName('Flowy_1.1.0_x64-setup.exe')).toEqual(['windows-x86_64']);
    expect(platformKeysForArtifactName('Flowy_1.1.0_aarch64-setup.exe')).toEqual(['windows-aarch64']);
  });

  test('maps Linux AppImages to arch keys', () => {
    expect(platformKeysForArtifactName('Flowy_1.1.0_amd64.AppImage')).toEqual(['linux-x86_64']);
    expect(platformKeysForArtifactName('Flowy_1.1.0_x86_64.AppImage')).toEqual(['linux-x86_64']);
    expect(platformKeysForArtifactName('Flowy_1.1.0_aarch64.AppImage')).toEqual(['linux-aarch64']);
  });

  test('maps macOS updater tarballs', () => {
    expect(platformKeysForArtifactName('Flowy.app.tar.gz')).toEqual(['darwin-x86_64', 'darwin-aarch64']);
    expect(platformKeysForArtifactName('Flowy_1.1.0_aarch64.app.tar.gz')).toEqual(['darwin-aarch64']);
    expect(platformKeysForArtifactName('Flowy_1.1.0_x64.app.tar.gz')).toEqual(['darwin-x86_64']);
  });

  test('ignores non-updater names', () => {
    expect(platformKeysForArtifactName('latest.json')).toEqual([]);
    expect(platformKeysForArtifactName('channel.yml')).toEqual([]);
  });
});
