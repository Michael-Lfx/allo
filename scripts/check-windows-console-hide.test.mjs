import { describe, expect, test } from 'bun:test';

import {
  checkLspSource,
  checkMediaSource,
  checkShellTransportSource,
  checkWorktreeSource,
  findBareCommandSpawns,
} from './check-windows-console-hide.mjs';

describe('windows console-hide contract', () => {
  test('accepts worktree hide helper', () => {
    const source = `
fn git_command() -> Command {
    let mut cmd = Command::new("git");
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn is_git_repo(root: &Path) -> bool {
    git_command().arg("-C").arg(root).output().is_ok()
}
`;
    expect(checkWorktreeSource(source)).toEqual([]);
  });

  test('rejects bare git Command::new', () => {
    const source = `
fn is_git_repo(root: &Path) -> bool {
    Command::new("git").arg("-C").arg(root).output().is_ok()
}
`;
    expect(checkWorktreeSource(source).length).toBeGreaterThan(0);
  });

  test('accepts lsp spawn with creation_flags', () => {
    const source = `
let mut cmd = tokio::process::Command::new(program);
cmd.args(args);
#[cfg(windows)]
{
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}
let mut child = cmd.spawn()?;
`;
    expect(checkLspSource(source)).toEqual([]);
  });

  test('rejects lsp spawn without hide', () => {
    const source = `
let mut child = tokio::process::Command::new(program)
    .args(args)
    .spawn()?;
`;
    expect(checkLspSource(source).length).toBeGreaterThan(0);
  });

  test('accepts media_command helper', () => {
    const source = `
fn media_command(bin: impl AsRef<Path>) -> Command {
    let mut cmd = Command::new(bin.as_ref());
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

async fn run(ffmpeg: &Path) {
    let _ = media_command(ffmpeg).output().await;
}
`;
    expect(checkMediaSource(source)).toEqual([]);
  });

  test('rejects bare ffmpeg Command::new', () => {
    const source = `
async fn run(ffmpeg: &Path) {
    let _ = Command::new(ffmpeg).output().await;
}
`;
    expect(checkMediaSource(source).length).toBeGreaterThan(0);
  });

  test('requires Windows shell_transport to force Pipe', () => {
    const good = `
pub(crate) fn shell_transport(requested_tty: bool) -> Transport {
    if cfg!(windows) {
        return Transport::Pipe;
    }
    if requested_tty {
        Transport::Pty { cols: 120, rows: 30 }
    } else {
        Transport::Pipe
    }
}
`;
    expect(checkShellTransportSource(good)).toEqual([]);

    const bad = `
pub(crate) fn shell_transport(requested_tty: bool) -> Transport {
    if cfg!(windows) || requested_tty {
        Transport::Pty { cols: 120, rows: 30 }
    } else {
        Transport::Pipe
    }
}
`;
    expect(checkShellTransportSource(bad).length).toBeGreaterThan(0);
  });

  test('findBareCommandSpawns ignores hide helpers', () => {
    const source = `
fn media_command(bin: &Path) -> Command {
    let mut cmd = Command::new(bin);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}
fn other() {
    Command::new("git");
}
`;
    const hits = findBareCommandSpawns(source, { helperNameRe: /media_command/ });
    expect(hits).toHaveLength(1);
    expect(hits[0].snippet).toContain('Command::new("git")');
  });
});
