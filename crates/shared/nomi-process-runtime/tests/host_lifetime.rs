#![cfg(any(unix, windows))]

use std::{
    fs,
    path::Path,
    process::Child,
    thread,
    time::{Duration, Instant},
};

use nomi_process_runtime::{
    hidden_std_command, install_host_process_lifetime, terminate_host_lifetime_descendants,
};

fn helper_binary() -> &'static str {
    env!("CARGO_BIN_EXE_process_test_helper")
}

fn harness_binary() -> &'static str {
    env!("CARGO_BIN_EXE_host_lifetime_harness")
}

#[test]
#[serial_test::serial]
fn terminate_kills_hidden_std_child_and_grandchild() {
    install_host_process_lifetime();
    let directory = tempfile::tempdir().expect("temporary directory");
    let grandchild_marker = directory.path().join("grandchild.pid");
    let mut child = hidden_std_command(helper_binary())
        .arg("spawn-grandchild")
        .arg(&grandchild_marker)
        .spawn()
        .expect("hidden helper should spawn");
    let leader_pid = child.id();
    let grandchild_pid = wait_for_pid_marker(&grandchild_marker);
    terminate_host_lifetime_descendants();
    wait_until_dead(leader_pid, "leader");
    wait_until_dead(grandchild_pid, "grandchild");
    reap_child(&mut child);
}

#[test]
#[serial_test::serial]
fn abrupt_harness_exit_kills_hidden_std_child_and_grandchild() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let leader_marker = directory.path().join("leader.pid");
    let grandchild_marker = directory.path().join("grandchild.pid");
    let start_gate = directory.path().join("start.gate");
    let exit_gate = directory.path().join("exit.gate");

    let mut harness = std::process::Command::new(harness_binary())
        .arg(helper_binary())
        .arg(&leader_marker)
        .arg(&grandchild_marker)
        .arg(&start_gate)
        .arg(&exit_gate)
        .spawn()
        .expect("host-lifetime harness should spawn");

    fs::write(&start_gate, b"go").expect("start gate");
    let leader_pid = wait_for_pid_marker(&leader_marker);
    let grandchild_pid = wait_for_pid_marker(&grandchild_marker);
    fs::write(&exit_gate, b"go").expect("exit gate");

    let status = wait_child_exit(&mut harness, Duration::from_secs(5), "harness");
    assert!(
        status.success(),
        "harness failed before abrupt exit: {status:?}"
    );
    wait_until_dead(leader_pid, "leader");
    wait_until_dead(grandchild_pid, "grandchild");
}

fn wait_for_pid_marker(path: &Path) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(contents) = fs::read_to_string(path)
            && let Ok(pid) = contents.trim().parse::<u32>()
        {
            return pid;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("PID marker was not published: {}", path.display());
}

fn wait_until_dead(pid: u32, label: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if !process_is_running(pid) {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("{label} pid={pid} remained alive after host-lifetime cleanup");
}

fn wait_child_exit(child: &mut Child, timeout: Duration, label: &str) -> std::process::ExitStatus {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            Ok(None) => panic!("{label} did not exit within {timeout:?}"),
            Err(error) => panic!("{label} wait failed: {error}"),
        }
    }
}

fn reap_child(child: &mut Child) {
    let _ = child.try_wait();
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(windows)]
fn process_is_running(pid: u32) -> bool {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_TIMEOUT},
        System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    };

    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        return false;
    }
    let wait = unsafe { WaitForSingleObject(handle, 0) };
    let _ = unsafe { CloseHandle(handle) };
    wait == WAIT_TIMEOUT
}

#[cfg(unix)]
fn process_is_running(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    let result = unsafe { libc::kill(pid, 0) };
    if result == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}
