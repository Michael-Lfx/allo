//! Host-lifetime binding for unmanaged helper processes on Unix.
//!
//! Each hidden spawn becomes a process-group leader. Linux children also set
//! `PR_SET_PDEATHSIG` so a killed client delivers `SIGKILL` to the leader.
//! A sibling `/bin/sh` reaper watches the client pid and `kill -KILL -$pgid`
//! if the client disappears while the group still has members. Graceful
//! shutdown walks every recorded PGID.

use std::{
    io,
    os::unix::process::CommandExt,
    process::{Child as StdChild, Command as StdCommand, Stdio},
    sync::{Mutex, OnceLock},
};

static HOST_GROUPS: OnceLock<Mutex<Vec<libc::pid_t>>> = OnceLock::new();

pub(crate) fn install_host_process_lifetime() {
    let _ = host_groups();
}

pub(crate) fn terminate_host_lifetime_descendants() {
    let Ok(groups) = host_groups().lock() else {
        return;
    };
    for pgid in groups.iter().copied() {
        // SAFETY: `pgid` was recorded as the process-group leader of a hidden
        // spawn owned by this process. ESRCH means the group is already gone.
        if unsafe { libc::kill(-pgid, libc::SIGKILL) } == -1 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                tracing::debug!(pgid, %error, "host-lifetime process group terminate failed");
            }
        }
    }
}

pub(crate) fn spawn_host_lifetime_std(
    command: &mut StdCommand,
) -> io::Result<StdChild> {
    bind_std_command(command);
    let child = command.spawn()?;
    commit_spawned_group(child.id())?;
    Ok(child)
}

pub(crate) fn spawn_host_lifetime_tokio(
    command: &mut tokio::process::Command,
) -> io::Result<tokio::process::Child> {
    command.process_group(0);
    bind_pre_exec(command.as_std_mut());
    let child = command.spawn()?;
    let pid = child.id().ok_or_else(|| {
        io::Error::other("child process exited before host-lifetime group registration")
    })?;
    commit_spawned_group(pid)?;
    Ok(child)
}

fn host_groups() -> &'static Mutex<Vec<libc::pid_t>> {
    HOST_GROUPS.get_or_init(|| Mutex::new(Vec::new()))
}

fn bind_std_command(command: &mut StdCommand) {
    command.process_group(0);
    bind_pre_exec(command);
}

fn bind_pre_exec(command: &mut StdCommand) {
    // SAFETY: the closure only calls async-signal-safe prctl/getppid/raise.
    unsafe {
        command.pre_exec(install_parent_death_signal);
    }
}

fn install_parent_death_signal() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) } == -1 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { libc::getppid() } == 1 {
            unsafe {
                libc::raise(libc::SIGKILL);
            }
        }
    }
    Ok(())
}

fn commit_spawned_group(pid: u32) -> io::Result<()> {
    let pgid = libc::pid_t::try_from(pid).map_err(|_| {
        io::Error::other(format!("child pid {pid} does not fit pid_t"))
    })?;
    match host_groups().lock() {
        Ok(mut groups) => groups.push(pgid),
        Err(_) => {
            tracing::error!("host-lifetime process group registry is poisoned");
        }
    }
    spawn_parent_death_reaper(pgid);
    Ok(())
}

fn spawn_parent_death_reaper(pgid: libc::pid_t) {
    let parent = std::process::id();
    let script = format!(
        "parent={parent}; pgid={pgid}; \
         while kill -0 \"$parent\" 2>/dev/null && kill -0 \"$pgid\" 2>/dev/null; do \
           sleep 0.2; \
         done; \
         if kill -0 \"$parent\" 2>/dev/null; then exit 0; fi; \
         kill -s KILL -$pgid 2>/dev/null || true"
    );
    let result = StdCommand::new("/bin/sh")
        .args(["-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Err(error) = result {
        tracing::debug!(pgid, %error, "host-lifetime parent-death reaper did not start");
    }
}
