//! Host-lifetime Job for unmanaged helper processes (`ffmpeg`, compositor
//! `node`, `ffprobe`, …) spawned outside [`crate::ProcessSupervisor`].
//!
//! The current process is intentionally **not** assigned to this Job: bash/PTY
//! trees already own nested Jobs, and self-assignment fails when the host is
//! already in a Debugger/cargo Job. Children are assigned while still
//! `CREATE_SUSPENDED` so grandchildren inherit membership. Closing this Job
//! handle — including kernel teardown when the client is killed — runs
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.

use std::{
    io,
    os::windows::{io::AsRawHandle, process::CommandExt},
    process::Child as StdChild,
    sync::OnceLock,
};

use windows_sys::Win32::{
    Foundation::HANDLE,
    System::{
        JobObjects::{AssignProcessToJobObject, TerminateJobObject},
        Threading::{
            CREATE_NO_WINDOW, CREATE_SUSPENDED, TerminateProcess, WaitForSingleObject,
        },
    },
};

use super::{
    TERMINATED_BY_HOST_EXIT_CODE, create_process_job, handles::OwnedHandle,
    resume_child_process_primary_thread,
};

struct HostLifetimeJob {
    job: OwnedHandle,
}

enum HostLifetime {
    Job(HostLifetimeJob),
    Unavailable,
}

static HOST: OnceLock<HostLifetime> = OnceLock::new();

pub(crate) fn install_host_process_lifetime() {
    let _ = host_lifetime();
}

pub(crate) fn terminate_host_lifetime_descendants() {
    let Some(job) = host_job_handle() else {
        return;
    };
    // SAFETY: `job` is the process-owned host-lifetime Job handle; the current
    // process is not a member, so this cannot terminate the client itself.
    if unsafe { TerminateJobObject(job, TERMINATED_BY_HOST_EXIT_CODE) } == 0 {
        let error = io::Error::last_os_error();
        tracing::warn!(
            %error,
            "host-lifetime Job terminate failed; kill-on-close still applies at process exit"
        );
    }
}

pub(crate) fn spawn_host_lifetime_std(
    command: &mut std::process::Command,
) -> io::Result<StdChild> {
    let Some(job) = host_job_handle() else {
        return command.spawn();
    };
    command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
    let mut child = command.spawn()?;
    if let Err(error) = assign_and_resume_std(job, &child) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    Ok(child)
}

pub(crate) fn spawn_host_lifetime_tokio(
    command: &mut tokio::process::Command,
) -> io::Result<tokio::process::Child> {
    let Some(job) = host_job_handle() else {
        return command.spawn();
    };
    command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
    let mut child = command.spawn()?;
    if let Err(error) = assign_and_resume_tokio(job, &child) {
        terminate_tokio_child(&mut child);
        return Err(error);
    }
    Ok(child)
}

fn host_lifetime() -> &'static HostLifetime {
    HOST.get_or_init(|| match create_process_job() {
        Ok(job) => HostLifetime::Job(HostLifetimeJob { job }),
        Err(error) => {
            tracing::error!(
                %error,
                "failed to create host-lifetime process Job; unmanaged helpers will outlive the client"
            );
            HostLifetime::Unavailable
        }
    })
}

fn host_job_handle() -> Option<HANDLE> {
    match host_lifetime() {
        HostLifetime::Job(host) => Some(host.job.as_raw()),
        HostLifetime::Unavailable => None,
    }
}

fn assign_and_resume_std(job: HANDLE, child: &StdChild) -> io::Result<()> {
    let pid = child.id();
    let process = child.as_raw_handle() as HANDLE;
    assign_suspended_child(job, pid, process)
}

fn assign_and_resume_tokio(job: HANDLE, child: &tokio::process::Child) -> io::Result<()> {
    let pid = child.id().ok_or_else(|| {
        io::Error::other("child process exited before host-lifetime Job assignment")
    })?;
    let process = child.raw_handle().ok_or_else(|| {
        io::Error::other("child process handle disappeared before host-lifetime Job assignment")
    })? as HANDLE;
    assign_suspended_child(job, pid, process)
}

fn assign_suspended_child(job: HANDLE, pid: u32, process: HANDLE) -> io::Result<()> {
    if process.is_null() {
        return Err(io::Error::other(
            "child process handle is null before host-lifetime Job assignment",
        ));
    }
    // SAFETY: both handles remain live; the child is still suspended so it
    // cannot create grandchildren before assignment completes.
    if unsafe { AssignProcessToJobObject(job, process) } == 0 {
        let assignment_error = io::Error::last_os_error();
        let _ = unsafe { TerminateProcess(process, TERMINATED_BY_HOST_EXIT_CODE) };
        let _ = unsafe { WaitForSingleObject(process, 5_000) };
        return Err(io::Error::new(
            assignment_error.kind(),
            format!("assign child process to host-lifetime Job: {assignment_error}"),
        ));
    }
    if let Err(error) = resume_child_process_primary_thread(pid) {
        let _ = unsafe { TerminateProcess(process, TERMINATED_BY_HOST_EXIT_CODE) };
        let _ = unsafe { WaitForSingleObject(process, 5_000) };
        return Err(io::Error::new(
            error.kind(),
            format!("resume host-lifetime child process: {error}"),
        ));
    }
    Ok(())
}

fn terminate_tokio_child(child: &mut tokio::process::Child) {
    if let Some(process) = child.raw_handle() {
        let process = process as HANDLE;
        if !process.is_null() {
            // SAFETY: Tokio still owns this exact process handle.
            let _ = unsafe { TerminateProcess(process, TERMINATED_BY_HOST_EXIT_CODE) };
            let _ = unsafe { WaitForSingleObject(process, 5_000) };
            return;
        }
    }
    let _ = child.start_kill();
}
