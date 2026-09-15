import { describe, expect, test } from 'bun:test';

import {
  checkCallSiteUsesRuntime,
  checkRuntimeOwnsHide,
  checkShellTransportSource,
} from './check-windows-console-hide.mjs';

describe('windows console-hide contract', () => {
  test('runtime must own hide helpers', () => {
    const good = `
pub fn apply_hidden_console(command: &mut Command) {
    #[cfg(windows)]
    {
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
}
pub fn apply_hidden_console_std(command: &mut std::process::Command) {}
pub fn hidden_command(program: impl AsRef<OsStr>) -> Command { Command::new(program) }
pub fn hidden_std_command(program: impl AsRef<OsStr>) -> std::process::Command {
    std::process::Command::new(program)
}
`;
    expect(checkRuntimeOwnsHide(good)).toEqual([]);
  });

  test('call sites must use runtime helpers without local flags', () => {
    const good = `
fn git_command() -> Command {
    nomi_process_runtime::hidden_std_command("git")
}
`;
    expect(checkCallSiteUsesRuntime(good)).toEqual([]);

    const bad = `
fn git_command() -> Command {
    let mut cmd = Command::new("git");
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}
`;
    expect(checkCallSiteUsesRuntime(bad).length).toBeGreaterThan(0);
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
});
