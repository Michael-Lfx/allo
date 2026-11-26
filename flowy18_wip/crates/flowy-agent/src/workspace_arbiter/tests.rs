use super::*;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "flowy-arbiter-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn directory(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn claim(path: &Path, access: WorkspaceAccess) -> WorkspaceRequest {
    WorkspaceRequest::Scoped(vec![
        WorkspaceClaim::resolve(path, access).unwrap(),
    ])
}

#[test]
fn reads_share_parent_child_ranges_but_writes_exclude_them() {
    let root = Scratch::new();
    let parent = root.directory("project");
    let child = root.directory("project/nested");
    let sibling = root.directory("project-other");
    let read = claim(&parent, WorkspaceAccess::Read);
    let nested_read = claim(&child, WorkspaceAccess::Read);
    let write = claim(&child, WorkspaceAccess::Write);
    assert!(!read.conflicts_with(&nested_read));
    assert!(read.conflicts_with(&write));
    assert!(!write.conflicts_with(&claim(&sibling, WorkspaceAccess::Write)));
    assert!(claim(&parent, WorkspaceAccess::Write).covers(&nested_read));
    assert!(!nested_read.covers(&write));
}

#[test]
fn path_aliases_are_the_same_resource() {
    let root = Scratch::new();
    let path = root.directory("project/nested");
    let alias = path.join("..").join("nested");
    assert!(
        claim(&path, WorkspaceAccess::Write)
            .conflicts_with(&claim(&alias, WorkspaceAccess::Read))
    );
    #[cfg(windows)]
    {
        let uppercase = PathBuf::from(path.to_string_lossy().to_uppercase());
        assert!(
            claim(&path, WorkspaceAccess::Write)
                .conflicts_with(&claim(&uppercase, WorkspaceAccess::Read))
        );
    }
}

#[cfg(any(unix, windows))]
#[test]
fn directory_link_alias_and_real_descendants_overlap() {
    let root = Scratch::new();
    let target = root.directory("target");
    let child = root.directory("target/child");
    let alias = root.0.join("alias");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &alias).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        let output = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&alias)
            .arg(&target)
            .creation_flags(0x0800_0000)
            .output()
            .unwrap();
        assert!(output.status.success(), "junction failed: {output:?}");
    }
    assert!(
        claim(&alias, WorkspaceAccess::Write)
            .conflicts_with(&claim(&child, WorkspaceAccess::Read))
    );
    fs::remove_dir(&alias)
        .or_else(|_| fs::remove_file(&alias))
        .unwrap();
}

#[test]
fn actual_tool_names_and_skill_reads_determine_the_complete_request() {
    let root = Scratch::new();
    let workspace = root.directory("workspace");
    let skill = root.directory("skill");
    let read = WorkspaceRequest::for_tools(&workspace, &["read_file".into()])
        .unwrap()
        .with_skill_reads([skill.clone()])
        .unwrap();
    assert!(!read.writes());
    assert!(read.conflicts_with(&claim(&skill, WorkspaceAccess::Write)));
    for tool in [
        "bash",
        "powershell",
        "verify_deck",
        "verify_artifact",
        "publish_file",
        "unclassified_extension",
    ] {
        assert!(matches!(
            WorkspaceRequest::for_tools(&workspace, &[tool.into()]).unwrap(),
            WorkspaceRequest::DeviceWrite,
        ));
    }
    assert!(
        WorkspaceRequest::for_tools(&workspace, &["write_file".into()])
            .unwrap()
            .writes()
    );
    assert!(
        WorkspaceRequest::for_tools(
            &root.0.join("missing"),
            &["read_file".into()]
        )
        .is_err()
    );
}

#[test]
fn full_resource_sets_and_capacity_are_granted_atomically() {
    let root = Scratch::new();
    let a = root.directory("a");
    let b = root.directory("b");
    let c = root.directory("c");
    let coordinator = StartCoordinator::in_memory();
    let both = |first: &Path, second: &Path| {
        WorkspaceRequest::Scoped(vec![
            WorkspaceClaim::resolve(first, WorkspaceAccess::Write).unwrap(),
            WorkspaceClaim::resolve(second, WorkspaceAccess::Write).unwrap(),
        ])
    };
    let first = coordinator
        .try_acquire(ExecutionClass::Foreground, both(&a, &b))
        .unwrap()
        .unwrap();
    let waiting = coordinator.enqueue(ExecutionClass::Background, both(&b, &a));
    assert!(waiting.take_if_ready(Instant::now()).unwrap().is_none());
    let disjoint = coordinator
        .try_acquire(
            ExecutionClass::Foreground,
            claim(&c, WorkspaceAccess::Read),
        )
        .unwrap()
        .unwrap();
    assert_eq!(coordinator.shared.state.lock().unwrap().active.len(), 2);
    disjoint.complete();
    first.complete();
    waiting
        .take_if_ready(Instant::now())
        .unwrap()
        .unwrap()
        .complete();
    assert!(coordinator.shared.state.lock().unwrap().active.is_empty());
}

#[test]
fn one_background_slot_leaves_capacity_for_a_disjoint_foreground() {
    let root = Scratch::new();
    let a = root.directory("a");
    let b = root.directory("b");
    let coordinator = StartCoordinator::in_memory();
    let first = coordinator
        .try_acquire(
            ExecutionClass::Background,
            claim(&a, WorkspaceAccess::Write),
        )
        .unwrap()
        .unwrap();
    assert!(
        coordinator
            .try_acquire(
                ExecutionClass::Background,
                claim(&b, WorkspaceAccess::Write)
            )
            .unwrap()
            .is_none()
    );
    let foreground = coordinator
        .try_acquire(
            ExecutionClass::Foreground,
            claim(&b, WorkspaceAccess::Write),
        )
        .unwrap()
        .unwrap();
    assert!(
        coordinator
            .try_acquire(
                ExecutionClass::Foreground,
                WorkspaceRequest::Scoped(vec![])
            )
            .unwrap()
            .is_none()
    );
    foreground.complete();
    first.complete();
}

#[tokio::test]
async fn cancelled_waiting_candidate_never_holds_capacity_or_resources() {
    let root = Scratch::new();
    let workspace = root.directory("project");
    let coordinator = StartCoordinator::in_memory();
    let active = coordinator
        .try_acquire(ExecutionClass::Foreground, WorkspaceRequest::DeviceWrite)
        .unwrap()
        .unwrap();
    let cancel = CancelToken::new();
    let candidate = coordinator.enqueue(
        ExecutionClass::Background,
        claim(&workspace, WorkspaceAccess::Write),
    );
    assert_eq!(coordinator.shared.state.lock().unwrap().active.len(), 1);
    cancel.cancel();
    assert_eq!(
        candidate.wait(&cancel).await.unwrap_err(),
        AdmissionError::Cancelled
    );
    assert!(coordinator.shared.state.lock().unwrap().pending.is_empty());
    active.complete();
    coordinator
        .try_acquire(
            ExecutionClass::Background,
            claim(&workspace, WorkspaceAccess::Write),
        )
        .unwrap()
        .unwrap()
        .complete();
}

#[tokio::test]
async fn shutdown_and_cancel_after_a_ticket_do_not_leak_writers() {
    let coordinator = StartCoordinator::in_memory();
    let active = coordinator
        .try_acquire(ExecutionClass::Foreground, WorkspaceRequest::DeviceWrite)
        .unwrap()
        .unwrap();
    let candidate = coordinator
        .enqueue(ExecutionClass::Background, WorkspaceRequest::DeviceWrite);
    coordinator.close();
    assert_eq!(
        candidate.wait(&CancelToken::new()).await.unwrap_err(),
        AdmissionError::Closed
    );
    active.complete();
    assert!(coordinator.shared.state.lock().unwrap().pending.is_empty());
    assert!(coordinator.shared.state.lock().unwrap().active.is_empty());
}

#[test]
fn foreground_priority_is_bounded_and_cannot_starve_a_waiting_background() {
    let coordinator = StartCoordinator::in_memory();
    let background = coordinator
        .enqueue(ExecutionClass::Background, WorkspaceRequest::DeviceWrite);
    for _ in 0..PRIORITY_MAX_BYPASSES {
        let foreground = coordinator
            .enqueue(ExecutionClass::Foreground, WorkspaceRequest::DeviceWrite);
        foreground
            .take_if_ready(Instant::now())
            .unwrap()
            .unwrap()
            .complete();
    }
    let foreground = coordinator
        .enqueue(ExecutionClass::Foreground, WorkspaceRequest::DeviceWrite);
    assert!(foreground.take_if_ready(Instant::now()).unwrap().is_none());
    background
        .take_if_ready(Instant::now())
        .unwrap()
        .unwrap()
        .complete();
    foreground
        .take_if_ready(Instant::now())
        .unwrap()
        .unwrap()
        .complete();
}

#[test]
fn unfinished_writer_survives_restart_as_an_explicit_quarantine() {
    let root = Scratch::new();
    let coordinator = StartCoordinator::new(&root.0);
    let writer = coordinator
        .try_acquire(ExecutionClass::Foreground, WorkspaceRequest::DeviceWrite)
        .unwrap()
        .unwrap()
        .started();
    assert!(root.0.join("execution-in-progress.lock").is_file());
    assert_eq!(
        coordinator
            .clear_device_quarantine(true)
            .unwrap_err()
            .kind(),
        io::ErrorKind::WouldBlock
    );
    drop(writer);
    assert!(coordinator.is_quarantined());
    let reopened = StartCoordinator::new(&root.0);
    assert!(reopened.is_quarantined());
    assert_eq!(
        reopened
            .try_acquire(
                ExecutionClass::Background,
                WorkspaceRequest::Scoped(vec![])
            )
            .unwrap_err(),
        AdmissionError::Quarantined
    );
    assert_eq!(
        reopened.clear_device_quarantine(false).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    let queued_writer = reopened
        .enqueue(ExecutionClass::Background, WorkspaceRequest::DeviceWrite);
    assert_eq!(
        reopened.clear_device_quarantine(true).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    drop(queued_writer);
    reopened.clear_device_quarantine(true).unwrap();
    let clean = reopened
        .try_acquire(ExecutionClass::Foreground, WorkspaceRequest::DeviceWrite)
        .unwrap()
        .unwrap();
    clean.complete();
    assert!(!root.0.join("execution-in-progress.lock").exists());
    assert!(!StartCoordinator::new(&root.0).is_quarantined());
}

#[test]
fn the_last_real_writer_removes_the_marker_and_failed_marker_io_denies_start() {
    let root = Scratch::new();
    let a = root.directory("a");
    let b = root.directory("b");
    let coordinator = StartCoordinator::new(&root.0);
    let first = coordinator
        .try_acquire(
            ExecutionClass::Foreground,
            claim(&a, WorkspaceAccess::Write),
        )
        .unwrap()
        .unwrap();
    let second = coordinator
        .try_acquire(
            ExecutionClass::Background,
            claim(&b, WorkspaceAccess::Write),
        )
        .unwrap()
        .unwrap();
    first.complete();
    assert!(root.0.join("execution-in-progress.lock").exists());
    second.complete();
    assert!(!root.0.join("execution-in-progress.lock").exists());
    let unavailable = StartCoordinator::new(&root.0.join("missing"));
    assert_eq!(
        unavailable
            .try_acquire(
                ExecutionClass::Foreground,
                WorkspaceRequest::DeviceWrite
            )
            .unwrap_err(),
        AdmissionError::Storage
    );
    assert!(unavailable.shared.state.lock().unwrap().active.is_empty());
    assert!(unavailable.is_quarantined());
}

fn tool_context(root: &Path) -> flowy_agent_core::ToolRunContext {
    flowy_agent_core::ToolRunContext {
        signal: CancelToken::new(),
        agent_id: Some("quiescence-fixture".into()),
        cwd: Some(root.display().to_string()),
        todos: None,
        submission_id: None,
        session_origin: flowy_agent_core::SessionOrigin::Home,
        execution: flowy_agent_core::ExecutionMetadata::conversation(),
        output_root: None,
        artifact_delivery_available: false,
    }
}

#[tokio::test]
async fn rejected_process_dispatch_does_not_require_a_quiescence_confirmation()
{
    use flowy_agent_core::tools::{
        ToolDefinition, ToolPolicyDecision, ToolRegistry,
    };
    let root = Scratch::new();
    let registry = ToolRegistry::new();
    registry.register(Arc::new(ToolDefinition::new(
        "unproven_extension",
        "A test-only unknown process capability",
        serde_json::json!({"type":"object","properties":{}}),
        |_, _| Box::pin(async { panic!("denied body must not run") }),
    )));
    let dispatched =
        crate::execution_host::track_unproven_process_dispatches(&registry);
    assert!(!dispatched.load(std::sync::atomic::Ordering::Acquire));
    registry.set_policy(Arc::new(|_| {
        ToolPolicyDecision::Deny("test denial".into())
    }));
    let result = registry
        .execute(
            "denied-call".into(),
            "unproven_extension".into(),
            serde_json::json!({}),
            tool_context(&root.0),
        )
        .await;
    assert!(result.is_error());
    assert!(!dispatched.load(std::sync::atomic::Ordering::Acquire));
    let coordinator = StartCoordinator::new(&root.0);
    let permit = coordinator
        .try_acquire(ExecutionClass::Foreground, WorkspaceRequest::DeviceWrite)
        .unwrap()
        .unwrap()
        .started();
    permit.settle(dispatched.load(std::sync::atomic::Ordering::Acquire));
    assert!(!coordinator.is_quarantined());
    assert!(!root.0.join(EXECUTION_MARKER).exists());
}

struct BackgroundWriterRelease {
    release: PathBuf,
    done: PathBuf,
}

impl Drop for BackgroundWriterRelease {
    fn drop(&mut self) {
        let _ = fs::write(&self.release, b"finish isolated fixture");
        // This child is created by this test and exits naturally. Never scan
        // or kill unrelated host processes, including when an assertion fails.
        for _ in 0..300 {
            if self.done.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[tokio::test]
async fn normal_native_shell_with_a_live_descendant_retains_device_quarantine()
{
    use flowy_agent_core::tools::{ToolRegistry, pi_shell_definition};
    let root = Scratch::new();
    let release = root.0.join("release-child");
    let started = root.0.join("child-started");
    let done = root.0.join("child-finished");
    let cleanup = BackgroundWriterRelease {
        release: release.clone(),
        done: done.clone(),
    };
    #[cfg(windows)]
    let command = {
        let literal =
            |path: &Path| path.display().to_string().replace('\'', "''");
        let child = format!(
            "[IO.File]::WriteAllText('{}','started'); $until=[DateTime]::UtcNow.AddSeconds(8); while(-not [IO.File]::Exists('{}') -and [DateTime]::UtcNow -lt $until) {{ Start-Sleep -Milliseconds 20 }}; [IO.File]::WriteAllText('{}','finished')",
            literal(&started),
            literal(&release),
            literal(&done),
        );
        format!(
            "$script='{}'; $encoded=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($script)); Start-Process -FilePath powershell.exe -ArgumentList @('-NoProfile','-NonInteractive','-EncodedCommand',$encoded) -WindowStyle Hidden",
            child.replace('\'', "''"),
        )
    };
    #[cfg(not(windows))]
    let command = {
        let literal =
            |path: &Path| path.display().to_string().replace('\'', "'\"'\"'");
        format!(
            "(trap '' HUP; printf started > '{}'; n=0; while [ ! -f '{}' ] && [ \"$n\" -lt 400 ]; do n=$((n+1)); sleep 0.02; done; printf finished > '{}') >/dev/null 2>&1 &",
            literal(&started),
            literal(&release),
            literal(&done),
        )
    };
    let registry = ToolRegistry::new();
    let shell = pi_shell_definition(&root.0);
    let shell_name = shell.name.clone();
    registry.register(shell);
    let dispatched =
        crate::execution_host::track_unproven_process_dispatches(&registry);
    assert!(!dispatched.load(std::sync::atomic::Ordering::Acquire));
    let coordinator = StartCoordinator::new(&root.0);
    let permit = coordinator
        .try_acquire(
            ExecutionClass::Foreground,
            WorkspaceRequest::for_tools(&root.0, &registry.list()).unwrap(),
        )
        .unwrap()
        .unwrap()
        .started();
    let result = registry.execute("native-child-fixture".into(), shell_name,
        serde_json::json!({"command":command,"description":"Start a bounded isolated child writer","timeout":10}),
        tool_context(&root.0)).await;
    assert!(
        !result.is_error(),
        "native fixture shell failed: {result:?}"
    );
    assert!(dispatched.load(std::sync::atomic::Ordering::Acquire));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !started.exists() {
        assert!(Instant::now() < deadline, "child did not start");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        !done.exists(),
        "child must still be waiting after normal shell exit"
    );
    permit.settle(dispatched.load(std::sync::atomic::Ordering::Acquire));
    assert!(coordinator.is_quarantined());
    assert!(root.0.join(EXECUTION_MARKER).is_file());
    assert_eq!(
        coordinator
            .try_acquire(
                ExecutionClass::Background,
                WorkspaceRequest::Scoped(vec![])
            )
            .unwrap_err(),
        AdmissionError::Quarantined
    );
    assert!(StartCoordinator::new(&root.0).is_quarantined());
    fs::write(&release, b"finish isolated child").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done.exists() {
        assert!(Instant::now() < deadline, "child cleanup did not finish");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    drop(cleanup);
    coordinator.clear_device_quarantine(true).unwrap();
    assert!(!root.0.join(EXECUTION_MARKER).exists());
}
