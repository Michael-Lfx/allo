import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { describe, expect, test } from 'bun:test';

const script = fileURLToPath(new URL('./make-latest-json.mjs', import.meta.url));

describe('make:latest --from-dir', () => {
  test('rebuilds a windows channel manifest from flat signed artifacts', () => {
    const dir = mkdtempSync(join(tmpdir(), 'make-latest-from-dir-'));
    const x64 = 'Flowy_1.2.3_x64-setup.exe';
    const arm = 'Flowy_1.2.3_arm64-setup.exe';
    writeFileSync(join(dir, x64), 'x64-bin');
    writeFileSync(join(dir, `${x64}.sig`), 'x64-sig');
    writeFileSync(join(dir, arm), 'arm-bin');
    writeFileSync(join(dir, `${arm}.sig`), 'arm-sig');

    const out = join(dir, 'latest.json');
    const channelYml = join(dir, 'channel.yml');
    const result = spawnSync(
      process.execPath,
      [
        script,
        '--host',
        'modelscope',
        '--channel',
        'windows',
        '--version',
        '1.2.3',
        '--notes',
        'from-dir test',
        '--from-dir',
        dir,
        '--out',
        out,
        '--channel-yml',
        channelYml,
      ],
      { encoding: 'utf8' },
    );

    expect(result.status).toBe(0);
    const manifest = JSON.parse(readFileSync(out, 'utf8'));
    expect(manifest.version).toBe('1.2.3');
    expect(Object.keys(manifest.platforms).sort()).toEqual(['windows-aarch64', 'windows-x86_64']);
    expect(manifest.platforms['windows-x86_64'].signature).toBe('x64-sig');
    expect(manifest.platforms['windows-aarch64'].signature).toBe('arm-sig');
    expect(manifest.platforms['windows-x86_64'].url).toContain(x64);
    expect(manifest.platforms['windows-aarch64'].url).toContain(arm);
    expect(manifest.platforms['windows-x86_64'].url).toContain(
      'FilePath=allo/windows/v1.2.3/Flowy_1.2.3_x64-setup.exe',
    );
    expect(readFileSync(channelYml, 'utf8')).toContain('manifest: channels/windows/latest.json');
  });

  test('unibuddy manifest pointer uses unibuddy_channels and keeps allo artifact paths', () => {
    const dir = mkdtempSync(join(tmpdir(), 'make-latest-unibuddy-'));
    const x64 = 'UniBuddy_1.2.3_x64-setup.exe';
    writeFileSync(join(dir, x64), 'x64-bin');
    writeFileSync(join(dir, `${x64}.sig`), 'x64-sig');

    const out = join(dir, 'latest.json');
    const channelYml = join(dir, 'channel.yml');
    const result = spawnSync(
      process.execPath,
      [
        script,
        '--host',
        'modelscope',
        '--channel',
        'windows',
        '--product',
        'UniBuddy',
        '--version',
        '1.2.3',
        '--from-dir',
        dir,
        '--out',
        out,
        '--channel-yml',
        channelYml,
      ],
      { encoding: 'utf8' },
    );

    expect(result.status).toBe(0);
    const manifest = JSON.parse(readFileSync(out, 'utf8'));
    const url = manifest.platforms['windows-x86_64'].url;
    expect(manifest.notes).toBe('UniBuddy v1.2.3');
    expect(url).toContain('FilePath=allo/windows/v1.2.3/UniBuddy_1.2.3_x64-setup.exe');
    expect(url).not.toContain('unibuddy_channels');
    expect(readFileSync(channelYml, 'utf8')).toContain('manifest: unibuddy_channels/windows/latest.json');
  });

  test('rejects a known product paired with the wrong channel root', () => {
    const dir = mkdtempSync(join(tmpdir(), 'make-latest-unibuddy-mismatch-'));
    const artifact = 'UniBuddy_1.2.3_x64-setup.exe';
    writeFileSync(join(dir, artifact), 'x64-bin');
    writeFileSync(join(dir, `${artifact}.sig`), 'x64-sig');

    const result = spawnSync(
      process.execPath,
      [
        script,
        '--host',
        'modelscope',
        '--channel',
        'windows',
        '--product',
        'UniBuddy',
        '--channel-root',
        'channels',
        '--version',
        '1.2.3',
        '--from-dir',
        dir,
      ],
      { encoding: 'utf8' },
    );

    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain('UniBuddy 必须使用 --channel-root unibuddy_channels');
  });
});
