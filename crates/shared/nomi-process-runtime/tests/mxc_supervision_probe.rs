//! MXC integration feasibility probe, driven through the REAL `ProcessSupervisor`.
//!
//! Why this exists: the earlier feasibility work used a standalone PowerShell
//! harness that *reconstructed* `arm_process_job`. That is enough to establish
//! Windows Job semantics, but it does NOT prove that this crate's own supervision
//! path can host a `wxc-exec` child. This test closes that gap by putting
//! `wxc-exec.exe` through `ProcessSupervisor::start` unchanged.
//!
//! Gated on `DSH_MXC_WXC` (absolute path to wxc-exec.exe) and `DSH_MXC_POLICY`
//! (absolute path to an MXC policy JSON). When unset the test is a no-op, so it
//! never breaks a normal `cargo test` run.
//!
//! **MEASURED RESULT (2026-09-21, Windows 11 build 29671, MXC ca7ea12): PASSES.**
//! `nomi-process-runtime` supervises a `wxc-exec` child successfully --
//! `code: Some(0)`, stdout `NOMI_SUPERVISED_OK`, `CleanupReport { reaped: true, errors: [] }`.
//!
//! This CORRECTS an earlier conclusion: the D-A Job/UI conflict documented in
//! `docs/architecture/agent-harness-mxc-*` does NOT block this crate's current
//! path, because `arm_process_job` sets only `KILL_ON_JOB_CLOSE` and no UI limits.
//!
//! Caveat that still applies: the policy must set `ui.disable: false`, or the
//! sandboxed payload dies on runtime init (STATUS_DLL_INIT_FAILED) -- see the
//! second assertion.

#![cfg(windows)]

use std::{
    collections::BTreeMap,
    ffi::OsString,
    time::{Duration, Instant},
};

use nomi_process_runtime::{
    CapabilityPolicy, CommandSpec, NormalizedProcessRequest, OutputCursor, PollResult,
    ProcessOwner, ProcessPolicy, ProcessSupervisor, SupervisorConfig, Transport,
};

fn env_path(key: &str) -> Option<OsString> {
    std::env::var_os(key).filter(|v| !v.is_empty())
}

fn mxc_request(wxc: OsString, policy: OsString, cwd: &std::path::Path) -> NormalizedProcessRequest {
    NormalizedProcessRequest {
        owner: ProcessOwner::new(uuid::Uuid::now_v7(), uuid::Uuid::now_v7()),
        command: CommandSpec::Program {
            program: wxc,
            args: vec![policy],
        },
        cwd: cwd.to_path_buf(),
        env: BTreeMap::new(),
        transport: Transport::Pipe,
        policy: ProcessPolicy::default(),
        capability: CapabilityPolicy::local_owner(cwd.to_path_buf()),
    }
}

/// The real question: can `nomi-process-runtime` supervise a `wxc-exec` child?
/// If the Job this crate creates were incompatible with MXC, `start` would fail
/// or the child would die with a backend error.
#[tokio::test]
async fn wxc_exec_runs_as_a_supervised_child_process() {
    let (Some(wxc), Some(policy)) = (env_path("DSH_MXC_WXC"), env_path("DSH_MXC_POLICY")) else {
        eprintln!(
            "skipping: set DSH_MXC_WXC and DSH_MXC_POLICY to run the MXC supervision probe"
        );
        return;
    };

    let cwd = std::env::current_dir().expect("cwd");
    let supervisor = ProcessSupervisor::new(SupervisorConfig::default());
    let handle = supervisor
        .start(mxc_request(wxc, policy, &cwd))
        .await
        .expect("nomi-process-runtime must be able to launch wxc-exec under its own Job");

    // Collect the terminal outcome; MXC one-shot runs exit on their own.
    let outcome = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let poll = supervisor
                .poll(
                    &handle.owner,
                    &handle.session_id,
                    OutputCursor::START,
                    Instant::now() + Duration::from_secs(10),
                )
                .await;
            match poll {
                Ok(PollResult::Finished(outcome)) => break outcome,
                Ok(PollResult::Running { .. }) => continue,
                Err(error) => panic!("poll failed: {error}"),
            }
        }
    })
    .await
    .expect("wxc-exec should reach a terminal state within 60s");

    let text = format!("{outcome:?}");
    eprintln!("--- supervised wxc-exec outcome ---\n{text}\n-----------------------------------");

    // The specific way this can fail today: MXC's own job setup rejects the
    // environment it inherits from a supervising Job.
    assert!(
        !text.contains("WIN32_ERROR(50)"),
        "wxc-exec reported ERROR_NOT_SUPPORTED under this crate's Job -- the \
         Job/UI-limit incompatibility DOES apply to nomi-process-runtime:\n{text}"
    );
    assert!(
        !text.contains("STATUS_DLL_INIT_FAILED"),
        "wxc-exec died on runtime init (ui policy / env issue), not a Job conflict:\n{text}"
    );
}

/// Follow-up probe: does attaching a supervised child to an ADDITIONAL Job that
/// carries UI limits fail?
///
/// **This is NOT a reproduction of the D-A harness failure, and it does not
/// reproduce it.** Attaching after the child has already started is a different
/// sequence from the harness case (which assigned a still-SUSPENDED child before
/// resume). Measured here: the late attach SUCCEEDS ("nesting allowed"), so this
/// probe is inconclusive about the harness condition.
///
/// It is kept because the negative result is itself informative: it shows the
/// conflict is specific to the harness's ordering, not to "a Job with UI limits
/// exists somewhere in the chain". Reaching the real condition would require this
/// crate to set UI limits on its own Job BEFORE assignment -- a capability the
/// public API deliberately does not have.
#[tokio::test]
async fn late_attach_to_a_ui_limited_job_is_probed_but_does_not_reproduce_d_a() {
    if env_path("DSH_MXC_UI_LIMIT_CONTROL").is_none() {
        eprintln!("skipping: set DSH_MXC_UI_LIMIT_CONTROL=1 to run the UI-limit control");
        return;
    }
    let (Some(wxc), Some(policy)) = (env_path("DSH_MXC_WXC"), env_path("DSH_MXC_POLICY")) else {
        eprintln!("skipping: DSH_MXC_WXC / DSH_MXC_POLICY unset");
        return;
    };

    let cwd = std::env::current_dir().expect("cwd");

    // Replicate this crate's Job exactly (KILL_ON_JOB_CLOSE only), then ADD a UI
    // limit -- i.e. the hypothetical future where nomi sets UI restrictions.
    let job = create_kill_on_close_job_with_ui_limit(0x10)
        .expect("control Job should be creatable");

    let mut request = mxc_request(wxc, policy, &cwd);
    request.policy.deadline = None;

    let supervisor = ProcessSupervisor::new(SupervisorConfig::default());
    let started = supervisor.start(request).await;

    let handle = match started {
        Ok(handle) => handle,
        Err(error) => {
            eprintln!("start failed (acceptable for this control): {error}");
            unsafe { CloseHandle(job) };
            return;
        }
    };

    // Pull the live PID and attach it to the UI-limited job.
    let snapshot = supervisor
        .status(&handle.owner, &handle.session_id)
        .await
        .expect("status should succeed");
    let pid = snapshot.pid;

    let attach = unsafe {
        let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
        if process == 0 {
            None
        } else {
            let ok = AssignProcessToJobObject(job, process);
            let err = if ok == 0 { Some(std::io::Error::last_os_error()) } else { None };
            CloseHandle(process);
            Some(err)
        }
    };

    eprintln!(
        "[control] attach pid={pid} to UI-limited job -> {:?}",
        match &attach {
            Some(None) => "OK (nesting allowed!)".to_string(),
            Some(Some(e)) => format!("FAILED win32={:?} ({e})", e.raw_os_error()),
            None => "could not open process".to_string(),
        }
    );

    unsafe { CloseHandle(job) };
    let _ = supervisor.cancel(&handle.owner, &handle.session_id).await;
}

#[cfg(windows)]
unsafe extern "system" {
    fn CreateJobObjectW(attrs: *mut core::ffi::c_void, name: *const u16) -> isize;
    fn SetInformationJobObject(job: isize, class: i32, info: *mut core::ffi::c_void, len: u32) -> i32;
    fn AssignProcessToJobObject(job: isize, process: isize) -> i32;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
    fn CloseHandle(handle: isize) -> i32;
}

#[cfg(windows)]
const PROCESS_SET_QUOTA: u32 = 0x0100;
#[cfg(windows)]
const PROCESS_TERMINATE: u32 = 0x0001;

#[cfg(windows)]
fn create_kill_on_close_job_with_ui_limit(ui_mask: u32) -> std::io::Result<isize> {
    #[repr(C)]
    struct JobBasicLimit {
        per_process_user_time: i64,
        per_job_user_time: i64,
        limit_flags: u32,
        min_working_set: usize,
        max_working_set: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }
    #[repr(C)]
    struct IoCounters {
        read_ops: u64,
        write_ops: u64,
        other_ops: u64,
        read_bytes: u64,
        write_bytes: u64,
        other_bytes: u64,
    }
    #[repr(C)]
    struct JobExtendedLimit {
        basic: JobBasicLimit,
        io: IoCounters,
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory: usize,
        peak_job_memory: usize,
    }
    #[repr(C)]
    struct JobUiRestrictions {
        class: u32,
    }

    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x2000;
    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: i32 = 9;
    const JOB_OBJECT_BASIC_UI_RESTRICTIONS: i32 = 4;

    let job = unsafe { CreateJobObjectW(core::ptr::null_mut(), core::ptr::null()) };
    if job == 0 {
        return Err(std::io::Error::last_os_error());
    }

    let mut ext: JobExtendedLimit = unsafe { core::mem::zeroed() };
    ext.basic.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let len = core::mem::size_of::<JobExtendedLimit>() as u32;
    let ok = unsafe {
        SetInformationJobObject(
            job,
            JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
            (&mut ext as *mut JobExtendedLimit).cast(),
            len,
        )
    };
    if ok == 0 {
        let err = std::io::Error::last_os_error();
        unsafe { CloseHandle(job) };
        return Err(err);
    }

    let mut ui = JobUiRestrictions { class: ui_mask };
    let ok = unsafe {
        SetInformationJobObject(
            job,
            JOB_OBJECT_BASIC_UI_RESTRICTIONS,
            (&mut ui as *mut JobUiRestrictions).cast(),
            core::mem::size_of::<JobUiRestrictions>() as u32,
        )
    };
    if ok == 0 {
        let err = std::io::Error::last_os_error();
        unsafe { CloseHandle(job) };
        return Err(err);
    }

    Ok(job)
}
