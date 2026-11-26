use super::{AppHost, OwnedBootstrap};
use crate::app::{FlowyApp, GrantStatus, automation_access::current_time_ms};
use crate::session::SessionStore;
use flowy_agent::{AgentInitialization, ChatLine, TurnStatus};
use gpui::TestAppContext;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug)]
struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let root = std::env::temp_dir().join(format!(
                "flowy-app-host-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            match fs::create_dir(&root) {
                Ok(()) => return Self(root),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create isolated test directory: {error}"),
            }
        }
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn host(&self) -> AppHost {
        AppHost::with_mode(OwnedBootstrap::acquire(self.path()).unwrap(), false)
            .unwrap()
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn assert_locked(root: &Path) {
    assert_eq!(
        OwnedBootstrap::acquire(root).unwrap_err().kind(),
        io::ErrorKind::AlreadyExists,
    );
}

fn wait_until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !predicate() {
        assert!(Instant::now() < deadline, "timed out waiting for host work");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn ownership_rejects_same_root_and_path_alias_before_history_changes() {
    let root = TestRoot::new();
    fs::create_dir(root.path().join("alias")).unwrap();
    fs::create_dir(root.path().join("sessions")).unwrap();
    let history = root.path().join("sessions/history.json");
    fs::write(&history, b"preserve original bytes").unwrap();
    let owner = OwnedBootstrap::acquire(root.path()).unwrap();
    assert_locked(root.path());
    assert_locked(&root.path().join("alias/.."));
    assert_eq!(fs::read(&history).unwrap(), b"preserve original bytes");
    drop(owner);
    assert!(OwnedBootstrap::acquire(root.path()).is_ok());
}

#[cfg(windows)]
#[test]
fn junction_and_case_aliases_cannot_start_another_store_owner() {
    let root = TestRoot::new();
    let aliases = TestRoot::new();
    let junction = aliases.path().join("junction");
    let created = Command::new("cmd.exe")
        .arg("/c")
        .arg("mklink")
        .arg("/J")
        .arg(&junction)
        .arg(root.path())
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "create junction: {} {}",
        String::from_utf8_lossy(&created.stdout),
        String::from_utf8_lossy(&created.stderr),
    );
    let owner = OwnedBootstrap::acquire(root.path()).unwrap();
    assert_locked(&junction);
    assert_locked(&PathBuf::from(root.path().to_string_lossy().to_uppercase()));
    drop(owner);
    let alias_owner = OwnedBootstrap::acquire(&junction).unwrap();
    assert_locked(root.path());
    drop(alias_owner);
    fs::remove_dir(&junction).unwrap();
}

#[cfg(unix)]
#[test]
fn symlink_alias_cannot_start_another_store_owner() {
    let root = TestRoot::new();
    let aliases = TestRoot::new();
    let link = aliases.path().join("link");
    std::os::unix::fs::symlink(root.path(), &link).unwrap();
    let owner = OwnedBootstrap::acquire(root.path()).unwrap();
    assert_locked(&link);
    drop(owner);
    let alias_owner = OwnedBootstrap::acquire(&link).unwrap();
    assert_locked(root.path());
    drop(alias_owner);
    fs::remove_file(&link).unwrap();
}

#[test]
fn stale_lock_file_is_preserved_and_different_roots_are_independent() {
    let first = TestRoot::new();
    let second = TestRoot::new();
    let lock_path = first.path().join("instance.lock");
    fs::write(&lock_path, b"existing lock file").unwrap();
    let first_owner = OwnedBootstrap::acquire(first.path()).unwrap();
    let second_owner = OwnedBootstrap::acquire(second.path()).unwrap();
    assert_locked(first.path());
    assert_locked(second.path());
    drop(first_owner);
    assert_eq!(fs::read(&lock_path).unwrap(), b"existing lock file");
    let reopened = OwnedBootstrap::acquire(first.path()).unwrap();
    drop(reopened);
    assert_eq!(fs::read(&lock_path).unwrap(), b"existing lock file");
    drop(second_owner);
}

#[test]
fn invalid_data_root_reports_its_io_error() {
    let root = TestRoot::new();
    let file = root.path().join("a-file");
    fs::write(&file, b"not a directory").unwrap();
    assert!(OwnedBootstrap::acquire(file).is_err());
    assert!(!root.path().join("sessions").exists());
}

#[test]
fn host_without_windows_initializes_and_executes_on_its_runtime() {
    let root = TestRoot::new();
    let host = root.host();
    assert_eq!(host.initialization_state(), AgentInitialization::Ready);
    let agent = host.agent();
    agent.send("headless host regression").unwrap();
    wait_until(|| !agent.chat_history().status.is_busy());
    let history = agent.chat_history();
    assert_eq!(history.status, TurnStatus::Completed);
    assert!(history.rows.iter().any(|line| matches!(
        line, ChatLine::User { text } if text == "headless host regression"
    )));
    assert!(history.rows.iter().any(|line| matches!(
        line, ChatLine::Assistant { text } if !text.is_empty()
    )));
    assert!(root.path().join("sessions/history.json").is_file());
    assert_locked(root.path());
    drop(agent);
    drop(host);
    wait_until(|| OwnedBootstrap::acquire(root.path()).is_ok());
}

#[test]
fn shutdown_is_idempotent_and_preserves_unclaimed_work() {
    let root = TestRoot::new();
    let host = root.host();
    let agent = host.agent();
    let pending = agent.accept_deferred("resume after restart").unwrap();
    let shutdown = host.start_shutdown(super::QUIT_GRACE).unwrap();

    assert!(host.start_shutdown(super::QUIT_GRACE).is_none());
    assert_eq!(agent.send("new run"), Err("notice.agent_busy"));
    assert!(matches!(
        agent.accept_deferred("new receipt"),
        Err("notice.agent_busy")
    ));
    assert!(agent.run_accepted_once().unwrap().is_none());
    wait_until(|| shutdown.is_finished());
    host.inner
        .runtime
        .as_ref()
        .unwrap()
        .block_on(shutdown)
        .unwrap();
    assert_locked(root.path());
    assert_eq!(agent.pending_acceptances()[0].id, pending.id);
    drop(agent);
    drop(host);

    let reopened = root.host();
    assert_eq!(reopened.agent().pending_acceptances()[0].id, pending.id);
}

#[test]
fn initialization_distinguishes_quarantined_files_and_unavailable_storage() {
    for name in ["index", "history"] {
        let root = TestRoot::new();
        let sessions = root.path().join("sessions");
        fs::create_dir(&sessions).unwrap();
        fs::write(sessions.join(format!("{name}.json")), b"{invalid").unwrap();
        let host = root.host();
        assert_eq!(
            host.initialization_state(),
            AgentInitialization::Quarantined,
            "damaged {name} must remain distinguishable from clean startup",
        );
        let preserved = fs::read_dir(&sessions)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(&format!("{name}.corrupt-"))
            })
            .expect("damaged storage must be preserved");
        assert_eq!(fs::read(preserved).unwrap(), b"{invalid");
        drop(host);
    }
    let root = TestRoot::new();
    fs::write(root.path().join("sessions"), b"blocks history directory")
        .unwrap();
    let host = root.host();
    assert_eq!(
        host.initialization_state(),
        AgentInitialization::Unavailable
    );
    assert_eq!(
        host.agent().create_home_session(),
        Err("notice.session_storage_failed")
    );
    assert!(host.agent().list_sessions().is_empty());
    assert_eq!(
        fs::read(root.path().join("sessions")).unwrap(),
        b"blocks history directory"
    );
}

#[test]
fn missing_artifact_store_is_not_a_ready_initialization() {
    let root = TestRoot::new();
    fs::write(root.path().join("artifacts"), b"blocks artifact directory")
        .unwrap();
    let host = root.host();
    assert_eq!(
        host.initialization_state(),
        AgentInitialization::Unavailable
    );
    assert!(host.agent().artifact_store().is_err());
}

#[test]
fn agent_and_artifact_clones_retain_exclusive_store_ownership() {
    let root = TestRoot::new();
    let host = root.host();
    let agent = host.agent();
    let artifacts = agent.artifact_store().unwrap();
    drop(host);
    assert_locked(root.path());
    // This remains a real storage writer after the application handle drops.
    let session = agent.create_home_session().unwrap();
    assert_eq!(agent.active_session_id(), Some(session));
    drop(agent);
    assert_locked(root.path());
    assert_eq!(artifacts.list().unwrap().len(), 0);
    drop(artifacts);
    wait_until(|| OwnedBootstrap::acquire(root.path()).is_ok());
}

#[test]
fn dropping_host_inside_async_context_does_not_panic_or_keep_lock() {
    let root = TestRoot::new();
    let host = root.host();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async move { drop(host) });
    wait_until(|| OwnedBootstrap::acquire(root.path()).is_ok());
    runtime.block_on(async {});
}

#[test]
fn background_initialization_outlives_an_immediate_host_drop() {
    let root = TestRoot::new();
    let host =
        AppHost::start(OwnedBootstrap::acquire(root.path()).unwrap()).unwrap();
    let agent = host.agent();
    drop(host);
    assert_locked(root.path());
    wait_until(|| {
        agent.initialization_state() != AgentInitialization::Initializing
    });
    assert_eq!(agent.initialization_state(), AgentInitialization::Ready);
    assert_locked(root.path());
    drop(agent);
    wait_until(|| OwnedBootstrap::acquire(root.path()).is_ok());
}

fn lock_probe_command(root: &Path, mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .arg("--exact")
        .arg("app::app_host::tests::lock_process_probe")
        .arg("--test-threads=1")
        .env("FLOWY_TEST_APP_HOST_LOCK_ROOT", root)
        .env("FLOWY_TEST_APP_HOST_LOCK_MODE", mode);
    command
}

#[test]
fn lock_process_probe() {
    let Some(root) = std::env::var_os("FLOWY_TEST_APP_HOST_LOCK_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    match std::env::var("FLOWY_TEST_APP_HOST_LOCK_MODE")
        .unwrap()
        .as_str()
    {
        "locked" => {
            assert_locked(&root);
            assert!(!root.join("sessions").exists());
        }
        "free" => {
            let owner = OwnedBootstrap::acquire(&root).unwrap();
            assert_eq!(owner.root(), root);
        }
        "hold" => {
            let _owner = OwnedBootstrap::acquire(&root).unwrap();
            fs::write(root.join("child-holds-lock"), b"ready").unwrap();
            // A pipe keeps the helper alive; parent termination also closes it.
            io::stdin().read_to_end(&mut Vec::new()).unwrap();
        }
        other => panic!("unknown lock probe mode {other}"),
    }
}

#[test]
fn another_process_cannot_acquire_owned_root_and_can_after_release() {
    let root = TestRoot::new();
    let owner = OwnedBootstrap::acquire(root.path()).unwrap();
    let blocked = lock_probe_command(root.path(), "locked").output().unwrap();
    assert!(
        blocked.status.success(),
        "{}",
        String::from_utf8_lossy(&blocked.stdout)
    );
    drop(owner);
    let available = lock_probe_command(root.path(), "free").output().unwrap();
    assert!(
        available.status.success(),
        "{}",
        String::from_utf8_lossy(&available.stdout)
    );
}

#[test]
fn killed_owner_process_releases_lock_without_deleting_lock_file() {
    let root = TestRoot::new();
    let mut child = lock_probe_command(root.path(), "hold")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    wait_until(|| root.path().join("child-holds-lock").is_file());
    assert_locked(root.path());
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(root.path().join("instance.lock").is_file());
    assert!(OwnedBootstrap::acquire(root.path()).is_ok());
}

#[test]
fn invalid_grant_ledgers_preserve_foreground_host_and_original_bytes() {
    for bytes in [
        b"broken-grant-ledger".as_slice(),
        b"{\"format_version\":999,\"entries\":[]}".as_slice(),
    ] {
        let root = TestRoot::new();
        fs::create_dir(root.path().join("automation")).unwrap();
        let path = root.path().join("automation/grants.json");
        fs::write(&path, bytes).unwrap();
        let host = root.host();
        assert!(!host.automation_grants_available());
        assert_eq!(
            host.initialization_state(),
            flowy_agent::AgentInitialization::Ready
        );
        assert!(host.agent().create_home_session().is_ok());
        let user = flowy_auth::Session {
            access_token: "isolated-grant-account".into(),
            email: "grants@example.invalid".into(),
            display_name: "Grant test".into(),
            user_id: Some(91),
        };
        host.start_automation_account(&user);
        let (principal, generation) =
            host.automation_account_binding().unwrap();
        let mut request = crate::app::automation_grants::tests::request();
        request.principal = principal.clone();
        request.account_generation = generation.clone();
        let now = crate::app::automation_access::current_time_ms();
        request.requested_at_ms = now - 10;
        request.request_expires_at_ms = now + 60_000;
        request.grant_expires_at_ms = now + 3_600_000;
        assert!(matches!(
            host.decide_automation_grant(request, true),
            Err(crate::app::GrantError::Unavailable),
        ));
        assert!(matches!(
            host.automation_grants(&principal, &generation),
            Err(crate::app::GrantError::Unavailable),
        ));
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn host_grant_revocation_survives_reopen_without_implicit_reapproval() {
    let root = TestRoot::new();
    let host = root.host();
    assert!(host.automation_grants_available());
    let user = flowy_auth::Session {
        access_token: "isolated-grant-account".into(),
        email: "grants@example.invalid".into(),
        display_name: "Grant test".into(),
        user_id: Some(91),
    };
    host.start_automation_account(&user);
    let (principal, generation) = host.automation_account_binding().unwrap();
    assert_eq!(
        host.automation_grants(&principal, &generation)
            .unwrap()
            .len(),
        0
    );
    let mut request = crate::app::automation_grants::tests::request();
    request.principal = principal.clone();
    request.account_generation = generation.clone();
    let now = crate::app::automation_access::current_time_ms();
    request.requested_at_ms = now - 10;
    request.request_expires_at_ms = now + 60_000;
    request.grant_expires_at_ms = now + 3_600_000;
    let crate::app::GrantStatus::Approved(binding) =
        host.decide_automation_grant(request.clone(), true).unwrap()
    else {
        panic!("expected persisted grant");
    };
    assert_eq!(
        host.automation_grants(&principal, &generation).unwrap(),
        vec![binding.grant_id.clone()]
    );
    assert!(
        host.revoke_automation_grant(
            &principal,
            &generation,
            &binding.grant_id
        )
        .unwrap()
    );
    drop(host);
    wait_until(|| OwnedBootstrap::acquire(root.path()).is_ok());
    let reopened = root.host();
    reopened.start_automation_account(&user);
    let (principal, generation) =
        reopened.automation_account_binding().unwrap();
    assert!(matches!(
        reopened.automation_grant_binding(
            &principal,
            &generation,
            &binding.grant_id
        ),
        Err(crate::app::GrantError::Revoked),
    ));
    assert_eq!(
        reopened
            .automation_grant_status(&principal, &generation, request.clone())
            .unwrap(),
        Some(crate::app::GrantStatus::Revoked),
    );
    assert!(matches!(
        reopened.decide_automation_grant(request, true),
        Err(crate::app::GrantError::LeaseExpired),
    ));
}

#[test]
fn automation_gate_retains_exclusive_store_ownership_after_host_drop() {
    let root = TestRoot::new();
    let host = root.host();
    let user = flowy_auth::Session {
        access_token: "isolated-grant-owner-token".into(),
        email: "grants@example.invalid".into(),
        display_name: "Grant test".into(),
        user_id: Some(91),
    };
    host.start_automation_account(&user);
    let (principal, generation) = host.automation_account_binding().unwrap();
    let route = flowy_agent::ModelRoute {
        provider: "flowy".into(),
        model: "retained-grant-fixture".into(),
        base_url: "https://grant.example.invalid/v1".into(),
        ..flowy_agent::ModelRoute::default()
    };
    host.automation_authority().replace_routes(
        &generation,
        &user,
        &[flowy_agent::CatalogModel {
            id: route.model.clone(),
            endpoint: route.base_url.clone(),
            ..flowy_agent::CatalogModel::default()
        }],
        0,
    );
    let mut request = crate::app::automation_grants::tests::request();
    request.principal = principal.clone();
    request.account_generation = generation.clone();
    let now = crate::app::automation_access::current_time_ms();
    request.requested_at_ms = now - 10;
    request.request_expires_at_ms = now + 60_000;
    request.grant_expires_at_ms = now + 3_600_000;
    let crate::app::GrantStatus::Approved(binding) =
        host.decide_automation_grant(request.clone(), true).unwrap()
    else {
        panic!("expected persisted grant");
    };
    let execution = flowy_agent::ExecutionMetadata {
        owner: Some(flowy_agent::ExecutionOwner {
            authority: principal.authority().to_owned(),
            subject: principal.external_user_id().to_string(),
        }),
        source: flowy_agent::ExecutionSource::Automation {
            automation_id: "retained-automation".into(),
            run_id: "retained-run".into(),
            attempt_id: "retained-attempt".into(),
        },
    };
    let gate = host
        .automation_lease(crate::app::AutomationLeaseRequest {
            principal,
            account_generation: generation,
            execution: execution.clone(),
            route: route.clone(),
            capabilities: request.scope,
            grant: binding,
        })
        .unwrap();
    drop(host);
    assert_eq!(
        OwnedBootstrap::acquire(root.path()).unwrap_err().kind(),
        io::ErrorKind::AlreadyExists,
    );
    assert_eq!(
        gate.model_credential(&execution, &route).unwrap(),
        "isolated-grant-owner-token",
    );
    drop(gate);
    wait_until(|| OwnedBootstrap::acquire(root.path()).is_ok());
}

#[gpui::test]
fn application_global_reuses_alias_and_rejects_a_different_root(
    cx: &TestAppContext,
) {
    let root = TestRoot::new();
    let other = TestRoot::new();
    fs::create_dir(root.path().join("alias")).unwrap();
    cx.update(|cx| {
        let first = AppHost::for_root(root.path(), cx).unwrap();
        let alias =
            AppHost::for_root(&root.path().join("alias/.."), cx).unwrap();
        assert!(Arc::ptr_eq(&first.inner, &alias.inner));
        assert_eq!(
            AppHost::for_root(other.path(), cx).unwrap_err().kind(),
            io::ErrorKind::InvalidInput,
        );
        assert!(!other.path().join("instance.lock").exists());
        drop(first);
        drop(alias);
        assert_locked(root.path());
        drop(cx.remove_global::<AppHost>());
    });
    wait_until(|| OwnedBootstrap::acquire(root.path()).is_ok());
}

#[gpui::test]
fn windows_share_host_but_preserve_local_drafts_and_themes(
    cx: &mut TestAppContext,
) {
    let root = TestRoot::new();
    let first = cx.add_window(|window, cx| {
        FlowyApp::new(
            SessionStore::new(root.path().to_path_buf()),
            false,
            window,
            cx,
        )
    });
    let second = cx.add_window(|window, cx| {
        FlowyApp::new(
            SessionStore::new(root.path().to_path_buf()),
            false,
            window,
            cx,
        )
    });
    let first_host = first
        .update(cx, |app, window, cx| {
            let prompt = app.studio.read(cx).prompt.clone();
            prompt.update(cx, |prompt, cx| {
                prompt.set_value("first window draft", window, cx);
            });
            app.ui.theme.update(cx, |theme, cx| {
                theme.update(
                    flowy_ui::ThemeOptions {
                        mode: flowy_ui::ThemeMode::Dark,
                        ..flowy_ui::ThemeOptions::default()
                    },
                    false,
                    cx,
                );
            });
            app.host.clone()
        })
        .unwrap();
    second
        .update(cx, |app, window, cx| {
            assert!(Arc::ptr_eq(&first_host.inner, &app.host.inner));
            assert!(!app.ui.theme.read(cx).resolved().dark);
            let prompt = app.studio.read(cx).prompt.clone();
            assert_eq!(prompt.read(cx).value(), "");
            prompt.update(cx, |prompt, cx| {
                prompt.set_value("second window draft", window, cx);
            });
        })
        .unwrap();
    first
        .update(cx, |app, _, cx| {
            assert!(app.ui.theme.read(cx).resolved().dark);
            assert_eq!(
                app.studio.read(cx).prompt.read(cx).value(),
                "first window draft"
            );
        })
        .unwrap();
    let session = first_host.agent().create_home_session().unwrap();
    second
        .update(cx, |app, _, cx| {
            assert_eq!(
                app.studio.read(cx).agent.active_session_id().as_deref(),
                Some(session.as_str())
            );
            assert_eq!(
                app.studio.read(cx).prompt.read(cx).value(),
                "second window draft"
            );
        })
        .unwrap();
    let host_identity = Arc::downgrade(&first_host.inner);
    drop(first_host);
    first
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    second
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.windows().len(), 0);
    assert!(host_identity.upgrade().is_some());
    assert_locked(root.path());
    let reopened = cx.add_window(|window, cx| {
        FlowyApp::new(
            SessionStore::new(root.path().to_path_buf()),
            false,
            window,
            cx,
        )
    });
    reopened
        .update(cx, |app, _, cx| {
            assert!(Arc::ptr_eq(
                &host_identity.upgrade().unwrap(),
                &app.host.inner,
            ));
            assert!(
                app.studio
                    .read(cx)
                    .agent
                    .list_sessions()
                    .iter()
                    .any(|item| item.id == session)
            );
        })
        .unwrap();
    reopened
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    cx.update(|cx| drop(cx.remove_global::<AppHost>()));
    cx.run_until_parked();
    wait_until(|| OwnedBootstrap::acquire(root.path()).is_ok());
}

#[gpui::test]
fn windows_adopt_shared_model_and_reject_an_obsolete_catalog_refresh(
    cx: &mut TestAppContext,
) {
    use flowy_agent::{CatalogModel, Model};

    let root = TestRoot::new();
    // Construct both windows signed out so neither opens a network request.
    let first = cx.add_window(|window, cx| {
        FlowyApp::new(
            SessionStore::new(root.path().to_path_buf()),
            false,
            window,
            cx,
        )
    });
    let second = cx.add_window(|window, cx| {
        FlowyApp::new(
            SessionStore::new(root.path().to_path_buf()),
            false,
            window,
            cx,
        )
    });
    let session = flowy_auth::Session {
        access_token: "isolated-model-sharing-test-token".into(),
        email: "model-sharing@example.invalid".into(),
        display_name: "Model sharing test".into(),
        user_id: Some(83),
    };
    let (initial_revision, initial_catalog, initial_tiers) = first
        .update(cx, |app, _, cx| {
            assert!(app.auth.session.is_none());
            assert!(!app.catalog_loading);
            app.auth.session = Some(session.clone());
            app.prefs.model_account = app.model_account().unwrap();
            app.effective_tier = 1;
            app.studio.update(cx, |panel, _| {
                panel.catalog.replace(vec![
                    CatalogModel {
                        id: "AIPC-qwen3.8-flash".into(),
                        name: "Lite fixture".into(),
                        endpoint: "https://lite.example.invalid/v1".into(),
                        context_window: 32_768,
                        max_tokens: 2_048,
                        ..CatalogModel::default()
                    },
                    CatalogModel {
                        id: "AIPC-deepseek-v4.1-flash".into(),
                        name: "Max fixture".into(),
                        endpoint: "https://max.example.invalid/v1".into(),
                        context_window: 65_536,
                        max_tokens: 4_096,
                        ..CatalogModel::default()
                    },
                ]);
            });
            app.select_tier(Model::Lite, cx);
            assert_eq!(app.applied_tier, Some(Model::Lite));
            assert!(app.studio.read(cx).cloud_ready);
            (
                app.host.model_revision(),
                app.studio.read(cx).catalog.clone(),
                app.studio.read(cx).tiers.clone(),
            )
        })
        .unwrap();
    assert!(initial_revision > 0);
    second
        .update(cx, |app, _, cx| {
            assert!(app.auth.session.is_none());
            assert!(!app.catalog_loading);
            app.auth.session = Some(session);
            assert!(app.adopt_shared_model(cx));
            assert_eq!(app.applied_tier, Some(Model::Lite));
            assert_eq!(app.prefs.model, Model::Lite);
            assert_eq!(app.effective_tier, 1);
            let panel = app.studio.read(cx);
            assert!(panel.cloud_ready);
            assert!(!panel.catalog_loading);
            assert_eq!(panel.model, Model::Lite);
            assert_eq!(panel.catalog, initial_catalog);
            assert_eq!(panel.tiers, initial_tiers);
            assert_eq!(app.host.model_revision(), initial_revision);
            // Re-adoption updates a projection without applying another route.
            assert!(app.adopt_shared_model(cx));
            assert_eq!(app.host.model_revision(), initial_revision);
        })
        .unwrap();
    let (latest_revision, latest_catalog, latest_tiers) = first
        .update(cx, |app, _, cx| {
            app.select_tier(Model::Max, cx);
            assert_eq!(app.applied_tier, Some(Model::Max));
            let panel = app.studio.read(cx);
            assert_eq!(
                panel.catalog.selected_model().unwrap().endpoint,
                "https://max.example.invalid/v1",
            );
            (
                app.host.model_revision(),
                panel.catalog.clone(),
                panel.tiers.clone(),
            )
        })
        .unwrap();
    assert!(latest_revision > initial_revision);
    let persisted_before =
        fs::read(root.path().join("preferences.json")).unwrap();
    cx.run_until_parked();
    second
        .update(cx, |app, _, cx| {
            // Ordinary host notification updates an already open window; no
            // explicit adoption or catalog response drives this assertion.
            assert_eq!(app.prefs.model, Model::Max);
            assert_eq!(app.prefs.model_id, "AIPC-deepseek-v4.1-flash");
            assert_eq!(app.applied_tier, Some(Model::Max));
            let panel = app.studio.read(cx);
            assert!(panel.cloud_ready);
            assert_eq!(panel.model, Model::Max);
            assert_eq!(panel.catalog, latest_catalog);
            assert_eq!(panel.tiers, latest_tiers);
            assert_eq!(app.host.model_revision(), latest_revision);
        })
        .unwrap();
    second
        .update(cx, |app, _, cx| {
            assert!(!app.accept_model_refresh(initial_revision, cx));
            assert_eq!(app.prefs.model, Model::Max);
            assert_eq!(app.prefs.model_id, "AIPC-deepseek-v4.1-flash");
            assert_eq!(app.applied_tier, Some(Model::Max));
            let panel = app.studio.read(cx);
            assert!(panel.cloud_ready);
            assert!(!panel.catalog_loading);
            assert_eq!(panel.model, Model::Max);
            assert_eq!(panel.catalog, latest_catalog);
            assert_eq!(panel.tiers, latest_tiers);
            let shared = app.host.shared_model().unwrap();
            assert_eq!(shared.tier, Model::Max);
            assert_eq!(shared.catalog, latest_catalog);
            assert_eq!(app.host.model_revision(), latest_revision);
        })
        .unwrap();
    assert_eq!(
        fs::read(root.path().join("preferences.json")).unwrap(),
        persisted_before,
    );
    let persisted = crate::prefs::Preferences::load(root.path());
    assert_eq!(persisted.model, Model::Max);
    assert_eq!(persisted.model_id, "AIPC-deepseek-v4.1-flash");
    first
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    second
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    cx.update(|cx| drop(cx.remove_global::<AppHost>()));
    cx.run_until_parked();
    wait_until(|| OwnedBootstrap::acquire(root.path()).is_ok());
}
#[test]
fn account_changes_advance_result_viewer_generation() {
    // PA-A11: every account change, including a same-account re-login,
    // invalidates results loaded for the previous viewer. Unowned foreground
    // sessions keep their visibility across these transitions.
    let root = TestRoot::new();
    let host = root.host();
    let legacy = host.agent().create_home_session().unwrap();
    let visible = |host: &AppHost| {
        host.agent()
            .list_sessions()
            .iter()
            .any(|session| session.id == legacy)
    };
    let started = host.result_viewer_generation();
    assert!(started > 0, "startup installs the initial viewer");
    assert!(visible(&host));
    let a = flowy_auth::Session {
        access_token: "isolated-viewer-a".into(),
        email: "viewer-a@example.invalid".into(),
        display_name: "Viewer A".into(),
        user_id: Some(101),
    };
    let b = flowy_auth::Session {
        access_token: "isolated-viewer-b".into(),
        email: "viewer-b@example.invalid".into(),
        display_name: "Viewer B".into(),
        user_id: Some(102),
    };
    let mut previous = started;
    for step in [Some(&a), Some(&a), Some(&b), None] {
        match step {
            Some(session) => host.start_automation_account(session),
            None => host.clear_automation_account(),
        }
        let current = host.result_viewer_generation();
        assert!(current > previous, "viewer generation must advance");
        assert!(visible(&host));
        previous = current;
    }
}

#[test]
fn explicit_manual_host_runs_reminder_then_joins_and_releases_store() {
    use flowy_automation::*;
    let root = TestRoot::new();
    let host = root.host();
    assert!(
        !root.path().join("automation/automations.sqlite3").exists(),
        "normal host construction must not open automation storage before lifecycle startup"
    );
    host.start_automation_account(&flowy_auth::Session {
        access_token: "isolated-reminder-account".into(),
        email: "reminder@example.invalid".into(),
        display_name: "Reminder fixture".into(),
        user_id: Some(916),
    });
    let (principal, account_generation) =
        host.automation_account_binding().unwrap();
    let caller = CallerContext {
        principal,
        account_generation,
        source: CallerSource::UserInterface,
        grant_refs: vec![],
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let handle = host
            .start_retained_automation(retention::RetentionLimits::default())
            .await
            .unwrap();
        let now = crate::app::automation_access::current_time_ms();
        let create = CommandEnvelope::new(
            RequestKey::parse("host-reminder-create").unwrap(),
            now,
            now + 60_000,
            AutomationCommand::CreateManual {
                definition: Box::new(ManualAutomationInput {
                    schedule: None,
                    title: PrivateText::new("Isolated reminder"),
                    action: ActionSpec::Reminder {
                        text: PrivateText::new("A local reminder result"),
                    },
                    workspace: None,
                    capabilities: CapabilityRequest::default(),
                    model: None,
                    budget: None,
                }),
            },
        )
        .unwrap();
        let receipt = handle.command(&caller, create).await.unwrap();
        let CommandOutcome::Created {
            automation_id,
            revision,
        } = receipt.outcome
        else {
            panic!("definition was not durably created");
        };
        let start = CommandEnvelope::new(
            RequestKey::parse("host-reminder-run").unwrap(),
            now,
            now + 60_000,
            AutomationCommand::RunNow {
                automation_id,
                expected_revision: revision,
            },
        )
        .unwrap();
        let receipt = handle.command(&caller, start).await.unwrap();
        let CommandOutcome::RunQueued { run_id } = receipt.outcome else {
            panic!("reminder was not queued");
        };
        let until = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let result = handle
                .query(
                    &caller,
                    AutomationQuery::Run {
                        run_id: run_id.clone(),
                    },
                )
                .await
                .unwrap();
            let QuerySnapshot::Run(run) = result else {
                panic!("wrong query projection");
            };
            if run.phase == RunPhase::Settled {
                assert_eq!(run.outcome, Some(RunOutcome::Succeeded));
                assert_eq!(run.current_attempt_id, None);
                break;
            }
            assert!(
                tokio::time::Instant::now() < until,
                "host worker did not settle reminder"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        use flowy_automation::schedule::{MisfirePolicy, SchedulePolicy, TriggerSpec};
        let now = current_time_ms();
        let scheduled_at = now + 250;
        let booked = handle.command(&caller, CommandEnvelope::new(
            RequestKey::parse("host-scheduled-reminder-create").unwrap(),now,now+60_000,
            AutomationCommand::CreateManual { definition:Box::new(ManualAutomationInput {
                title:PrivateText::new("Scheduled reminder"),
                action:ActionSpec::Reminder { text:PrivateText::new("Run in the headless host") },
                schedule:Some(SchedulePolicy { trigger:TriggerSpec::Once {scheduled_at_ms:scheduled_at}, misfire:MisfirePolicy::Skip,
                    max_lateness_ms:std::num::NonZeroU64::new(60_000).unwrap() }),
                workspace:None, model:None, budget:None, capabilities:CapabilityRequest::default(),
            }) },
        ).unwrap()).await.unwrap();
        let CommandOutcome::Created { automation_id,.. } = booked.outcome else { panic!("scheduled definition") };
        tokio::time::timeout(Duration::from_secs(5),async {
            loop {
                let history = handle.query(&caller,AutomationQuery::History {automation_id:automation_id.clone()}).await.unwrap();
                if matches!(history,QuerySnapshot::History(runs) if runs.len()==1 && runs[0].scheduled_at_ms==Some(scheduled_at) && runs[0].outcome==Some(RunOutcome::Succeeded)) {
                    let QuerySnapshot::Inbox(cards) = handle.query(&caller,AutomationQuery::Inbox).await.unwrap() else { panic!("inbox") };
                    if cards.len()==2 { break; }
                }
                tokio::time::sleep(Duration::from_millis(40)).await;
            }
        }).await.expect("scheduled reminder and its inbox card settle without a window");
        host.stop_manual_automation().await.unwrap();
        assert!(matches!(
            handle.query(&caller, AutomationQuery::List).await,
            Err(PortError::Unavailable)
        ));
    });
    drop(runtime);
    drop(host);
    wait_until(|| OwnedBootstrap::acquire(root.path()).is_ok());
}

#[test]
fn conversation_caller_factory_checks_persisted_submission_and_account_generation()
 {
    let root = TestRoot::new();
    fs::create_dir_all(root.path().join("sessions")).unwrap();
    let history = serde_json::json!({
        "active": "caller-session",
        "sessions": [{
            "id": "caller-session",
            "origin": {"kind": "home"},
            "execution": {"owner": null, "source": {"kind": "conversation"}},
            "messages": [{"id": "submission-1", "role": "user", "source": {"kind": "user"}, "content": [{"type": "text", "text": "isolated caller test"}]}]
        }]
    });
    fs::write(
        root.path().join("sessions/history.json"),
        serde_json::to_vec(&history).unwrap(),
    )
    .unwrap();
    let host = root.host();
    let user = flowy_auth::Session {
        access_token: "isolated-caller-account".into(),
        email: "caller@example.invalid".into(),
        display_name: "Caller test".into(),
        user_id: Some(91),
    };
    host.start_automation_account(&user);
    let factory = host.automation_caller_factory(vec![]).unwrap();
    let mut context = flowy_agent::ToolRunContext {
        signal: flowy_agent::CancelToken::new(),
        agent_id: Some("caller-session".into()),
        cwd: None,
        todos: None,
        submission_id: Some("submission-1".into()),
        session_origin: flowy_agent::SessionOrigin::Home,
        execution: flowy_agent::ExecutionMetadata::conversation(),
        output_root: None,
        artifact_delivery_available: false,
    };
    let args = flowy_agent::ToolCallArgs {
        call_id: "call-1".into(),
        name: "automation_create".into(),
        arguments: serde_json::json!({}),
    };
    assert!(factory(&context, &args).is_ok());
    context.submission_id = Some("forged-submission".into());
    assert_eq!(
        factory(&context, &args),
        Err(flowy_automation::PortError::Forbidden)
    );
    context.submission_id = Some("submission-1".into());
    host.invalidate_automation_leases();
    assert_eq!(
        factory(&context, &args),
        Err(flowy_automation::PortError::Forbidden)
    );
    let fresh = host.automation_caller_factory(vec![]).unwrap();
    assert!(fresh(&context, &args).is_ok());
    let mut other = user;
    other.user_id = Some(92);
    host.start_automation_account(&other);
    assert_eq!(
        fresh(&context, &args),
        Err(flowy_automation::PortError::Forbidden)
    );
}

#[test]
fn approval_ledger_recovery_binds_original_receipt_and_rechecks_owner_and_revocation()
 {
    use super::super::automation_host::{approval_request, resolve_approval};
    use flowy_automation::store::{AutomationStore, ReceiptState};
    use flowy_automation::*;

    let root = TestRoot::new();
    let host = root.host();
    let mut user = flowy_auth::Session {
        access_token: "isolated-approval-account".into(),
        email: "approval@example.invalid".into(),
        display_name: "Approval fixture".into(),
        user_id: Some(917),
    };
    host.start_automation_account(&user);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let conversation = |host: &AppHost| {
        let (principal, account_generation) =
            host.automation_account_binding().unwrap();
        CallerContext {
            principal,
            account_generation,
            grant_refs: vec![],
            source: CallerSource::TrustedSubmission {
                session_id: SessionRef::parse("approval-source").unwrap(),
                submission_id: SubmissionRef::parse("approval-submission")
                    .unwrap(),
                call_id: "approval-tool-call".into(),
            },
        }
    };
    runtime.block_on(async {
        let store =
            AutomationStore::open(root.path().to_path_buf(), Arc::new(()))
                .await
                .unwrap();
        let owner = conversation(&host);
        let now = current_time_ms();
        let definition = ManualAutomationInput {
            title: PrivateText::new("Approved work"),
            action: ActionSpec::AgentTask {
                prompt: PrivateText::new("Draft the report"),
            },
            workspace: None,
            schedule: None,
            capabilities: CapabilityRequest {
                tools: vec![],
                resources: vec!["model:flowy/fixture".into()],
                grant_ref: None,
            },
            model: Some(ModelBinding {
                selected: ModelRef {
                    provider: "flowy".into(),
                    model: "fixture".into(),
                },
                allowed_fallbacks: vec![],
            }),
            budget: RunBudget::new(60_000, 2, 2),
        };
        for key in ["recover-original", "revoked-before-apply"] {
            let envelope = CommandEnvelope::new(
                RequestKey::parse(key).unwrap(),
                now,
                now + 60_000,
                AutomationCommand::CreateManual {
                    definition: Box::new(definition.clone()),
                },
            )
            .unwrap();
            let receipt = store
                .apply_command(
                    owner.clone(),
                    envelope.clone(),
                    now,
                    HostAccessDecision::Blocked(
                        BlockedReason::ApprovalRequired,
                    ),
                )
                .await
                .unwrap();
            assert_eq!(receipt.outcome, CommandOutcome::PendingApproval);
        }
        let cards = store
            .pending_approvals(owner.principal.clone(), now)
            .await
            .unwrap();
        assert_eq!(cards.len(), 2);
        let mut grants = Vec::new();
        for card in &cards {
            let GrantStatus::Approved(binding) = host
                .decide_automation_grant(approval_request(&owner, card), true)
                .unwrap()
            else {
                panic!("approved")
            };
            grants.push(binding.grant_id);
        }
        host.revoke_automation_grant(
            &owner.principal,
            &owner.account_generation,
            &grants[1],
        )
        .unwrap();
        store.shutdown().await.unwrap();
        let store =
            AutomationStore::open(root.path().to_path_buf(), Arc::new(()))
                .await
                .unwrap();

        user.user_id = Some(918);
        host.start_automation_account(&user);
        let other = conversation(&host);
        assert_eq!(
            store.pending_approvals(other.principal, now).await.unwrap(),
            Vec::new()
        );
        assert_eq!(
            resolve_approval(
                &store,
                &host.inner.grants,
                owner.clone(),
                cards[0].clone(),
                Some(true)
            )
            .await,
            Err(PortError::Forbidden)
        );

        user.user_id = Some(917);
        host.start_automation_account(&user);
        let restored = conversation(&host);
        for card in cards {
            resolve_approval(
                &store,
                &host.inner.grants,
                restored.clone(),
                card,
                None,
            )
            .await
            .unwrap();
        }
        let applied = store
            .receipt(
                owner.principal.clone(),
                RequestKey::parse("recover-original").unwrap(),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(applied.state, ReceiptState::Applied);
        let rejected = store
            .receipt(
                owner.principal.clone(),
                RequestKey::parse("revoked-before-apply").unwrap(),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rejected.state, ReceiptState::Rejected);
        let QuerySnapshot::Automations(items) = store
            .query(owner.principal, AutomationQuery::List)
            .await
            .unwrap()
        else {
            panic!("definitions")
        };
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].definition.capabilities.grant_ref.as_ref(),
            Some(&grants[0])
        );
        store.shutdown().await.unwrap();
    });
}
