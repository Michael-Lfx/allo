use super::submit::format_skill_prompt;
use super::*;
use crate::drawer_preview::DrawerPreviewBody;
use flowy_agent::{AgentWorkspace, FileEntry};
use gpui::TestAppContext;
use std::sync::atomic::Ordering;
use std::{cell::RefCell, rc::Rc};

/// Builds a panel inside its own window: `StudioPanel::new` needs the
/// window that owns its deferred work, and the tests only ever read state.
fn open_panel(
    cx: &TestAppContext,
    build: impl FnOnce(&mut Window, &mut Context<'_, StudioPanel>) -> StudioPanel,
) -> Entity<StudioPanel> {
    let mut panel = None;
    cx.update(|cx| {
        let slot = &mut panel;
        cx.open_window(gpui::WindowOptions::default(), move |window, cx| {
            let built = cx.new(|cx| build(window, cx));
            *slot = Some(built.clone());
            built
        })
        .expect("the test window opens");
    });
    panel.expect("the panel was built with its window")
}

/// The prompt text the user sees: queued programmatic text wins over the
/// editor state, which only receives it at the top of `render`.
fn visible_prompt(panel: &StudioPanel, cx: &gpui::App) -> String {
    match panel.prompt_pending.as_deref() {
        Some(pending) => pending.to_owned(),
        None => panel.prompt.read(cx).value().to_string(),
    }
}

fn submission_panel(
    cx: &mut TestAppContext,
    tag: &str,
) -> (Entity<StudioPanel>, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "flowy-studio-submit-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    let panel = open_panel(cx, |window, cx| {
        let ui = UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        StudioPanel::new(
            StudioConfig {
                product: "Flowy",
                i18n: I18n::embedded("Flowy"),
                model: Model::Lite,
                plan: Plan::Free,
                model_info_version: String::new(),
                model_info_open: false,
                store_root: root.clone(),
                llm: LlmConfig {
                    api_key: None,
                    ..LlmConfig::default()
                },
            },
            ui,
            window,
            cx,
        )
    });
    panel.update(cx, |panel, cx| {
        panel.catalog.replace(vec![flowy_agent::CatalogModel {
            id: "test-model".into(),
            name: "Test model".into(),
            ..Default::default()
        }]);
        panel.cloud_ready = true;
        cx.notify();
    });
    (panel, root)
}

#[gpui::test]
fn repeated_send_while_submitting_persists_one_user_message(
    cx: &mut TestAppContext,
) {
    let (panel, root) = submission_panel(cx, "duplicate");
    panel.update(cx, |panel, cx| {
        panel.fill_prompt("one request", cx);
        panel.send_prompt(cx);
        let id = panel.submission;
        panel.send_prompt(cx);
        assert_eq!(panel.submission, id);
        assert!(panel.submission_pending);
    });
    cx.run_until_parked();
    cx.read_entity(&panel, |panel, cx| {
        assert!(!panel.submission_pending);
        assert_eq!(panel.prompt.read(cx).value(), "");
        assert_eq!(
            panel
                .agent
                .chat_history()
                .rows
                .iter()
                .filter(|row| matches!(row, flowy_agent::ChatLine::User { .. }))
                .count(),
            1
        );
    });
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn a_busy_submission_shows_up_in_the_pending_strip(cx: &mut TestAppContext) {
    let (panel, root) = submission_panel(cx, "pending-strip");
    panel.update(cx, |panel, cx| {
        let id = panel.agent.create_home_session().unwrap();
        panel.open_session(&id, cx);
        panel.status = flowy_agent::TurnStatus::Running;
        panel.refresh_working(cx);
        assert_eq!(panel.pending_receipts.len(), 0);

        panel.fill_prompt("queued while the turn runs", cx);
        panel.send_prompt(cx);

        // The strip is rendered from this field, so a non-empty field is
        // what "the user can find it" means at the UI layer.
        assert_eq!(panel.pending_receipts.len(), 1);
        assert_eq!(
            panel.pending_receipts[0].text,
            "queued while the turn runs"
        );
        // The old lie is gone: no "added to this session's task list"
        // notice is raised for a list that is now real.
        assert_eq!(panel.notice, None);
    });

    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn a_restarted_panel_rebuilds_the_pending_strip_from_the_journal(
    cx: &mut TestAppContext,
) {
    // First run: accept while busy.
    let (panel, root) = submission_panel(cx, "pending-restart");
    let first_id = panel.update(cx, |panel, cx| {
        let id = panel.agent.create_home_session().unwrap();
        panel.open_session(&id, cx);
        panel.status = flowy_agent::TurnStatus::Running;
        panel.refresh_working(cx);
        panel.fill_prompt("survive the restart", cx);
        panel.send_prompt(cx);
        panel.pending_receipts[0].id.clone()
    });
    drop(panel);

    // Second run over the same store root: the constructor reads the
    // journal, so the strip is populated before anything is submitted.
    let reopened = open_panel(cx, |window, cx| {
        let ui = UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        StudioPanel::new(
            StudioConfig {
                product: "Flowy",
                i18n: I18n::embedded("Flowy"),
                model: Model::Lite,
                plan: Plan::Free,
                model_info_version: String::new(),
                model_info_open: false,
                store_root: root.clone(),
                llm: LlmConfig {
                    api_key: None,
                    ..LlmConfig::default()
                },
            },
            ui,
            window,
            cx,
        )
    });
    cx.read_entity(&reopened, |panel, _cx| {
        assert_eq!(
            panel.pending_receipts.len(),
            1,
            "a restart must still show what was accepted"
        );
        assert_eq!(panel.pending_receipts[0].id, first_id);
    });

    drop(reopened);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn a_restarted_panel_drains_the_queued_receipt_without_another_submit(
    cx: &mut TestAppContext,
) {
    // First run: accept while busy, then die with the receipt still queued.
    let (panel, root) = submission_panel(cx, "restart-drain");
    let receipt_id = panel.update(cx, |panel, cx| {
        let id = panel.agent.create_home_session().unwrap();
        panel.open_session(&id, cx);
        panel.status = flowy_agent::TurnStatus::Running;
        panel.refresh_working(cx);
        panel.fill_prompt("drain me after restart", cx);
        panel.send_prompt(cx);
        panel.pending_receipts[0].id.clone()
    });
    drop(panel);

    // Second run: nothing is submitted; startup alone must claim it.
    let reopened = open_panel(cx, |window, cx| {
        let ui = UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        StudioPanel::new(
            StudioConfig {
                product: "Flowy",
                i18n: I18n::embedded("Flowy"),
                model: Model::Lite,
                plan: Plan::Free,
                model_info_version: String::new(),
                model_info_open: false,
                store_root: root.clone(),
                llm: LlmConfig {
                    api_key: None,
                    ..LlmConfig::default()
                },
            },
            ui,
            window,
            cx,
        )
    });
    // Deliver the startup event the way the pump would after the agent
    // reports readiness; the panel must claim the waiting receipt itself.
    reopened.update(cx, |panel, cx| {
        panel.process_agent_updates(
            vec![flowy_agent::AgentUpdate::Initialized],
            cx,
        );
    });
    let queued = flowy_agent::AcceptanceQueue::new(root.clone())
        .all()
        .expect("acceptance journal readable");
    let receipt = queued
        .iter()
        .find(|receipt| receipt.id == receipt_id)
        .expect("the queued receipt is still on disk");
    assert!(
        receipt.taken_at_ms.is_some(),
        "startup must claim the queued receipt without a second submit"
    );

    drop(reopened);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn a_manual_submit_keeps_the_queue_waiting(cx: &mut TestAppContext) {
    let (panel, root) = submission_panel(cx, "drain-guard");
    let receipt_id = panel.update(cx, |panel, cx| {
        let id = panel.agent.create_home_session().unwrap();
        panel.open_session(&id, cx);
        panel.status = flowy_agent::TurnStatus::Running;
        panel.refresh_working(cx);
        panel.fill_prompt("wait for the submit", cx);
        panel.send_prompt(cx);
        let receipt_id = panel.pending_receipts[0].id.clone();
        panel.initialized = true;
        panel.submission_pending = true;
        panel.refresh_pending_receipts(cx);
        receipt_id
    });
    let queued = flowy_agent::AcceptanceQueue::new(root.clone())
        .all()
        .expect("acceptance journal readable");
    let receipt = queued
        .iter()
        .find(|receipt| receipt.id == receipt_id)
        .expect("the queued receipt is still on disk");
    assert!(
        receipt.taken_at_ms.is_none(),
        "a manual submit must not be interleaved with a queued receipt"
    );

    panel.update(cx, |panel, cx| {
        panel.submission_pending = false;
        panel.status = flowy_agent::TurnStatus::Idle;
        panel.refresh_pending_receipts(cx);
    });
    let queued = flowy_agent::AcceptanceQueue::new(root.clone())
        .all()
        .expect("acceptance journal readable");
    let receipt = queued
        .iter()
        .find(|receipt| receipt.id == receipt_id)
        .expect("the queued receipt is still on disk");
    assert!(
        receipt.taken_at_ms.is_some(),
        "once idle the queued receipt starts"
    );

    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn a_queued_receipt_names_the_session_it_belongs_to(cx: &mut TestAppContext) {
    let (panel, root) = submission_panel(cx, "pending-owner");
    panel.update(cx, |panel, cx| {
        let first = panel.agent.create_home_session().unwrap();
        panel.open_session(&first, cx);
        panel.status = flowy_agent::TurnStatus::Running;
        panel.refresh_working(cx);
        panel.fill_prompt("stay in this session", cx);
        panel.send_prompt(cx);
        let receipt = panel.pending_receipts[0].clone();
        assert_eq!(panel.receipt_owner(&receipt), panel.t("chat.pending_here"));

        // A receipt from another session is labelled by that session, not
        // by whichever one happens to be active now.
        let mut foreign = receipt;
        foreign.session_hint = Some("elsewhere".to_owned());
        let owner = panel.receipt_owner(&foreign);
        assert_ne!(owner, panel.t("chat.pending_here"));
        assert_ne!(owner, panel.t("chat.pending_unassigned"));
    });
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn sending_while_running_is_accepted_into_the_durable_queue(
    cx: &mut TestAppContext,
) {
    let (panel, root) = submission_panel(cx, "busy-accept");
    panel.update(cx, |panel, cx| {
        let id = panel.agent.create_home_session().unwrap();
        panel.open_session(&id, cx);
        // A turn is in flight: this is the state a user is in when the
        // send button used to disappear.
        panel.status = flowy_agent::TurnStatus::Running;
        panel.refresh_working(cx);

        panel.fill_prompt("second prompt while the first turn runs", cx);
        panel.send_prompt(cx);
    });

    // The submission must be durable, not just remembered.
    let pending = flowy_agent::AcceptanceQueue::new(root.clone())
        .pending()
        .expect("acceptance queue readable");
    assert_eq!(
        pending.len(),
        1,
        "busy submit must be accepted, not dropped"
    );
    assert_eq!(pending[0].text, "second prompt while the first turn runs");
    assert!(pending[0].taken_at_ms.is_none(), "nothing ran it yet");

    // The draft is handed over to the queue, so the composer is clear and
    // the user can keep typing.
    cx.read_entity(&panel, |panel, cx| {
        assert_eq!(panel.prompt.read(cx).value(), "");
    });

    // Reopening the client is a fresh queue handle over the same root:
    // no in-memory state is shared, only the receipt file.
    let reopened = flowy_agent::AcceptanceQueue::new(root.clone())
        .pending()
        .expect("reopened queue readable");
    assert_eq!(reopened.len(), 1);
    assert_eq!(reopened[0].id, pending[0].id);

    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn an_accepted_receipt_is_drained_when_the_turn_finishes(
    cx: &mut TestAppContext,
) {
    let (panel, root) = submission_panel(cx, "accepted-drain");
    panel.update(cx, |panel, cx| {
        let id = panel.agent.create_home_session().unwrap();
        panel.open_session(&id, cx);
        panel.status = flowy_agent::TurnStatus::Running;
        panel.refresh_working(cx);
        panel.fill_prompt("queued work", cx);
        panel.send_prompt(cx);
        assert_eq!(panel.pending_receipts.len(), 1);

        // The running turn ends: the refresh drains the receipt and starts
        // it in its hinted session.
        panel.refresh_chat(cx);
        assert!(panel.agent.has_active_claim());
        assert_eq!(panel.pending_receipts.len(), 0);

        for _ in 0..200 {
            if !panel.agent.turn_status().is_busy() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panel.refresh_chat(cx);
        assert!(!panel.agent.has_active_claim());
        assert_eq!(panel.pending_receipts.len(), 0);
    });
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn retrying_delivery_shows_saving_state_and_blocks_reentry(
    cx: &mut TestAppContext,
) {
    let (panel, root) = submission_panel(cx, "retry-delivery");
    panel.update(cx, |panel, cx| {
        let id = panel.agent.create_home_session().unwrap();
        panel.open_session(&id, cx);
        panel.status = flowy_agent::TurnStatus::DeliveryFailed;
        panel.retrying_delivery = Some(id.clone());
        panel.refresh_working(cx);

        let label = panel.working.read(cx).label_for_test().to_owned();
        assert_eq!(label, panel.t("chat.status_saving_files"));
        assert!(!panel.working.read(cx).retry_enabled());

        // A second activation while the background retry is in flight is
        // ignored instead of queueing another job behind the dispatcher.
        panel.retry_delivery(cx);
        assert_eq!(panel.retrying_delivery.as_deref(), Some(id.as_str()));
    });
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn failed_background_submission_keeps_draft_and_retry_identity(
    cx: &mut TestAppContext,
) {
    let (panel, root) = submission_panel(cx, "failure");
    let id = panel.update(cx, |panel, cx| {
        panel
            .agent
            .set_workspace_for_new_sessions(Some(AgentWorkspace {
                project_id: "unavailable".into(),
                root: root.join("missing-project"),
            }));
        panel.fill_prompt("keep for retry", cx);
        panel.send_prompt(cx);
        panel.submission.unwrap()
    });
    cx.run_until_parked();
    panel.update(cx, |panel, cx| {
        assert_eq!(panel.prompt.read(cx).value(), "keep for retry");
        assert_eq!(panel.submission, Some(id));
        assert!(!panel.agent.has_messages());
        assert_eq!(panel.notice, Some("project.folder_unavailable"));
        panel.send_prompt(cx);
        assert_eq!(panel.submission, Some(id));
    });
    cx.run_until_parked();
    cx.read_entity(&panel, |panel, cx| {
        assert_eq!(panel.prompt.read(cx).value(), "keep for retry");
        assert_eq!(panel.submission, Some(id));
        assert!(!panel.submission_pending);
    });
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn late_submission_does_not_clear_new_draft_or_change_selected_session(
    cx: &mut TestAppContext,
) {
    let (panel, root) = submission_panel(cx, "navigation");
    let next = panel.update(cx, |panel, cx| {
        let first = panel.agent.create_home_session().unwrap();
        let next = panel.agent.create_home_session().unwrap();
        panel.open_session(&first, cx);
        panel.fill_prompt("request for first", cx);
        panel.send_prompt(cx);
        panel.open_session(&next, cx);
        panel.fill_prompt("new session draft", cx);
        next
    });
    cx.run_until_parked();
    cx.read_entity(&panel, |panel, cx| {
        assert_eq!(
            panel.agent.active_session_id().as_deref(),
            Some(next.as_str())
        );
        assert_eq!(panel.prompt.read(cx).value(), "new session draft");
        assert!(!panel.agent.has_messages());
        assert_eq!(panel.surface, StudioSurface::Home);
    });
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn background_initialized_event_keeps_home_and_preserves_unsent_draft(
    cx: &mut TestAppContext,
) {
    let root = std::env::temp_dir().join(format!(
        "flowy-studio-init-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    std::fs::create_dir_all(root.join("sessions")).unwrap();
    std::fs::write(root.join("sessions/index.json"), r#"{"active":"restored","sessions":[{"id":"restored","title":"Saved conversation","updated_at":1,"lines":[{"kind":"user","text":"original request"},{"kind":"assistant","text":"Saved answer"}],"status":{"kind":"completed"},"origin":{"kind":"home"}}]}"#).unwrap();
    let panel = open_panel(cx, |window, cx| {
        let ui = UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        StudioPanel::new_background(
            StudioConfig {
                product: "Flowy",
                i18n: I18n::embedded("Flowy"),
                model: Model::Lite,
                plan: Plan::Free,
                model_info_version: String::new(),
                model_info_open: false,
                store_root: root.clone(),
                llm: LlmConfig {
                    api_key: None,
                    ..LlmConfig::default()
                },
            },
            ui,
            window,
            cx,
        )
    });
    let agent = cx.read_entity(&panel, |panel, _| panel.agent.clone());
    // The loader is a real worker thread (SQLite + official skill seeding),
    // so yielding the test thread lets it finish without advancing the GPUI
    // timer pump or fabricating events. Allow generous wall-clock headroom
    // for parallel NTFS/SQLite contention on Windows CI runners.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let mut updates = Vec::new();
    loop {
        updates.extend(agent.drain_updates());
        if !agent.is_initializing()
            && updates.iter().any(|update| {
                matches!(update, flowy_agent::AgentUpdate::Initialized)
            })
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "background session initialization did not finish in time (initializing={}, updates={updates:?})",
            agent.is_initializing(),
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        updates.iter().any(|update| matches!(
            update,
            flowy_agent::AgentUpdate::Initialized
        ))
    );
    panel.update(cx, |panel, cx| {
        panel.surface = StudioSurface::Home;
        panel.fill_prompt("keep this draft", cx);
        panel.process_agent_updates(updates, cx);
        assert_eq!(panel.notice, None);
        // Launching must not dive back into the last conversation just
        // because it still has history; the user asked for Home first.
        assert_eq!(panel.surface, StudioSurface::Home);
        assert_eq!(panel.agent.active_session_id().as_deref(), Some("restored"));
        // Programmatic fills are applied at the top of `render`; the test
        // reads the queued draft instead of driving a frame.
        assert_eq!(visible_prompt(panel, cx), "keep this draft");
        let history = panel.agent.chat_history();
        assert_eq!(history.rows.len(), 2);
        assert!(matches!(&history.rows[0], flowy_agent::ChatLine::User { text } if text == "original request"));
    });
    drop(panel);
    drop(agent);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn creating_a_project_persists_and_opens_its_workspace(
    cx: &mut TestAppContext,
) {
    let root = std::env::temp_dir().join(format!(
        "flowy-studio-project-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let folder = root.join("workspace");
    std::fs::create_dir_all(&folder).unwrap();
    let panel = open_panel(cx, |window, cx| {
        let ui = UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        StudioPanel::new(
            StudioConfig {
                product: "Flowy",
                i18n: I18n::embedded("Flowy"),
                model: Model::Lite,
                plan: Plan::Free,
                model_info_version: String::new(),
                model_info_open: false,
                store_root: root.clone(),
                llm: LlmConfig {
                    api_key: None,
                    base_url: String::new(),
                    model: String::new(),
                    provider: String::new(),
                    ..LlmConfig::default()
                },
            },
            ui,
            window,
            cx,
        )
    });
    let created = panel
        .update(cx, |panel, cx| panel.create_project("Client", &folder, cx))
        .unwrap();
    assert_eq!(crate::projects::load(&root).unwrap(), vec![created.clone()]);
    cx.read_entity(&panel, |panel, _| {
        assert_eq!(
            panel.active_project_id.as_deref(),
            Some(created.id.as_str())
        );
        assert_eq!(
            panel.agent.active_workspace(),
            Some(flowy_agent::AgentWorkspace {
                project_id: created.id.clone(),
                root: created.root.clone(),
            })
        );
    });

    let session = cx
        .read_entity(&panel, |panel, _| panel.agent.create_session().unwrap());
    panel.update(cx, |panel, cx| panel.open_session(&session, cx));
    cx.read_entity(&panel, |panel, _| {
        assert_eq!(
            panel.active_project_id.as_deref(),
            Some(created.id.as_str())
        );
        assert_eq!(
            panel
                .agent
                .active_workspace()
                .as_ref()
                .map(|value| &value.root),
            Some(&created.root)
        );
    });
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn persisted_error_is_visible_on_the_first_frame(cx: &TestAppContext) {
    let root = std::env::temp_dir().join(format!(
        "flowy-studio-working-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let sessions = root.join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    // The agent boots from its structured index, the panel projection from
    // the UI index; both must agree that the active session ended in error.
    std::fs::write(
        sessions.join("history.json"),
        r#"{"active":"s1","sessions":[{"id":"s1","status":{"kind":"error","message":"chat.status_error"}}]}"#,
    )
    .unwrap();
    std::fs::write(
        sessions.join("index.json"),
        r#"{"active":"s1","sessions":[{"id":"s1","title":"t","updated_at":1,"titled":true,"lines":[],"status":{"kind":"error","message":"chat.status_error"}}]}"#,
    )
    .unwrap();
    let panel = open_panel(cx, |window, cx| {
        let ui = UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        StudioPanel::new(
            StudioConfig {
                product: "Flowy",
                i18n: I18n::embedded("Flowy"),
                model: Model::Lite,
                plan: Plan::Free,
                model_info_version: String::new(),
                model_info_open: false,
                store_root: root.clone(),
                llm: LlmConfig {
                    api_key: None,
                    base_url: String::new(),
                    model: String::new(),
                    provider: String::new(),
                    ..LlmConfig::default()
                },
            },
            ui,
            window,
            cx,
        )
    });
    cx.read_entity(&panel, |panel, cx| {
        assert_eq!(
            panel.status,
            flowy_agent::TurnStatus::Error {
                message: "chat.status_error".into(),
            }
        );
        let working = panel.working.read(cx);
        assert!(working.is_visible());
        assert!(working.is_error());
        let expected = panel.i18n.t("chat.status_error");
        assert_ne!(expected, "chat.status_error");
        assert_eq!(working.label(), expected);
    });
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn chat_surface_enter_submits_through_the_chat_composer(
    cx: &mut TestAppContext,
) {
    let root = std::env::temp_dir().join(format!(
        "flowy-studio-chat-enter-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let (panel, cx) = cx.add_window_view(|window, cx| {
        let ui = UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        StudioPanel::new(
            StudioConfig {
                product: "Flowy",
                i18n: I18n::embedded("Flowy"),
                model: Model::Lite,
                plan: Plan::Free,
                model_info_version: String::new(),
                model_info_open: false,
                store_root: root.clone(),
                llm: LlmConfig {
                    api_key: None,
                    base_url: String::new(),
                    model: String::new(),
                    provider: String::new(),
                    ..LlmConfig::default()
                },
            },
            ui,
            window,
            cx,
        )
    });
    cx.update(|window, cx| {
        // The composer's editor advertises the `Input` key context, the
        // same one the app binds its submit chord against.
        cx.bind_keys([gpui::KeyBinding::new(
            "enter",
            crate::SubmitComposer,
            Some("Input"),
        )]);
        panel.update(cx, |panel, cx| {
            panel.catalog.replace(vec![flowy_agent::CatalogModel {
                id: "mock-model".into(),
                name: "Mock model".into(),
                ..Default::default()
            }]);
            panel.cloud_ready = true;
            panel.set_surface(StudioSurface::Chat, cx);
            panel.fill_prompt("hello", cx);
        });
        use gpui::Focusable;
        let handle = panel.read(cx).prompt.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    });
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.read_entity(&panel, |panel, cx| {
        // Submitting queues the cleared draft for the next render.
        assert_eq!(visible_prompt(panel, cx), "");
        assert!(panel.agent.has_messages());
    });
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn enter_submits_the_draft_and_shift_enter_inserts_a_newline(
    cx: &mut TestAppContext,
) {
    let root = std::env::temp_dir().join(format!(
        "flowy-studio-enter-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let (panel, cx) = cx.add_window_view(|window, cx| {
        let ui = UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        StudioPanel::new(
            StudioConfig {
                product: "Flowy",
                i18n: I18n::embedded("Flowy"),
                model: Model::Lite,
                plan: Plan::Free,
                model_info_version: String::new(),
                model_info_open: false,
                store_root: root.clone(),
                llm: LlmConfig {
                    api_key: None,
                    base_url: String::new(),
                    model: String::new(),
                    provider: String::new(),
                    ..LlmConfig::default()
                },
            },
            ui,
            window,
            cx,
        )
    });
    cx.update(|window, cx| {
        // The app installs these in `flowy_app::app::run`; a studio-only
        // test has to install the same bindings to exercise the handler.
        cx.bind_keys([
            gpui::KeyBinding::new(
                "enter",
                crate::SubmitComposer,
                Some("Input"),
            ),
            gpui::KeyBinding::new(
                "shift-enter",
                gpui_kit::base::input::Enter {
                    secondary: false,
                    shift: true,
                },
                Some("Input"),
            ),
        ]);
        panel.update(cx, |panel, cx| {
            panel.catalog.replace(vec![flowy_agent::CatalogModel {
                id: "mock-model".into(),
                name: "Mock model".into(),
                ..Default::default()
            }]);
            panel.cloud_ready = true;
            panel.fill_prompt("hello", cx);
        });
        use gpui::Focusable;
        let handle = panel.read(cx).prompt.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    });
    // Home surface: Shift+Enter inserts a newline, Enter submits.
    cx.simulate_keystrokes("shift-enter");
    cx.read_entity(&panel, |panel, cx| {
        assert_eq!(visible_prompt(panel, cx), "hello\n");
    });
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.read_entity(&panel, |panel, cx| {
        // Submitting queues the cleared draft for the next render.
        assert_eq!(visible_prompt(panel, cx), "");
        assert!(panel.agent.has_messages());
    });

    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn failed_turn_keeps_the_working_line_visible() {
    use flowy_agent::TurnStatus;

    assert!(working_line_visible(&TurnStatus::Running));
    assert!(working_line_visible(&TurnStatus::Stopping));
    assert!(working_line_visible(&TurnStatus::WaitingForUser));
    assert!(working_line_visible(&TurnStatus::Error {
        message: "chat.status_error".into(),
    }));
    assert!(working_line_visible(&TurnStatus::DeliveryFailed));
    assert!(!working_line_visible(&TurnStatus::Idle));
    assert!(!working_line_visible(&TurnStatus::Completed));
    assert!(!working_line_visible(&TurnStatus::Stopped));
}

#[gpui::test]
fn background_session_updates_do_not_change_selected_surface(
    cx: &TestAppContext,
) {
    let root = std::env::temp_dir().join(format!(
        "flowy-studio-navigation-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let panel = open_panel(cx, |window, cx| {
        let ui = UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        StudioPanel::new(
            StudioConfig {
                product: "Flowy",
                i18n: I18n::embedded("Flowy"),
                model: Model::Lite,
                plan: Plan::Free,
                model_info_version: String::new(),
                model_info_open: false,
                store_root: root.clone(),
                llm: LlmConfig {
                    api_key: None,
                    base_url: String::new(),
                    model: String::new(),
                    provider: String::new(),
                    ..LlmConfig::default()
                },
            },
            ui,
            window,
            cx,
        )
    });
    let notices = Rc::new(RefCell::new(Vec::new()));
    let subscription = cx.update(|cx| {
        cx.subscribe(&panel, {
            let notices = notices.clone();
            move |_, event: &StudioEvent, _| {
                if let StudioEvent::Notice(notice) = event {
                    notices.borrow_mut().push(*notice);
                }
            }
        })
    });
    cx.update(|cx| {
        panel.update(cx, |panel, cx| {
            panel.catalog.replace(vec![flowy_agent::CatalogModel {
                id: "test-model".into(),
                name: "Test model".into(),
                ..Default::default()
            }]);
            panel.cloud_ready = true;
            panel.fill_prompt("hello", cx);
            panel.send_prompt(cx);
            assert!(panel.submission_pending);
            panel.set_surface(StudioSurface::Automation, cx);
        });
    });
    cx.run_until_parked();
    cx.update(|cx| {
        panel.update(cx, |panel, cx| {
            let updates = panel.agent.drain_updates();
            panel.process_agent_updates(updates, cx);
            assert_eq!(panel.surface, StudioSurface::Automation);
            panel.process_agent_updates(
                vec![flowy_agent::AgentUpdate::StorageError],
                cx,
            );
            assert_eq!(panel.notice, Some("notice.session_storage_failed"));
            assert_eq!(panel.surface, StudioSurface::Automation);
        });
    });
    cx.update(|cx| panel.update(cx, StudioPanel::new_task));
    assert_eq!(notices.borrow().last(), Some(&None));
    drop(subscription);
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}
#[gpui::test]
fn theme_changes_preserve_draft_and_do_not_leak_between_panels(
    cx: &TestAppContext,
) {
    let root = std::env::temp_dir()
        .join(format!("flowy-studio-theme-{}", std::process::id()));
    let (first_ui, second_ui, theme) = cx.update(|cx| {
        let first_ui =
            UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        let second_ui =
            UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        let theme = first_ui.theme.clone();
        (first_ui, second_ui, theme)
    });
    let panel_config = |root: &PathBuf| StudioConfig {
        product: "Flowy",
        i18n: I18n::embedded("Flowy"),
        model: Model::Lite,
        plan: Plan::Free,
        model_info_version: String::new(),
        model_info_open: false,
        store_root: root.clone(),
        llm: LlmConfig {
            api_key: None,
            base_url: String::new(),
            model: String::new(),
            provider: String::new(),
            ..LlmConfig::default()
        },
    };
    let first = open_panel(cx, {
        let config = panel_config(&root);
        move |window, cx| StudioPanel::new(config, first_ui, window, cx)
    });
    let second = open_panel(cx, {
        let config = panel_config(&root);
        move |window, cx| StudioPanel::new(config, second_ui, window, cx)
    });
    cx.update(|cx| {
        first.update(cx, |panel, cx| {
            panel.fill_prompt("preserve draft", cx);
        });
    });
    cx.update(|cx| {
        theme.update(cx, |theme, cx| {
            theme.update(
                flowy_ui::ThemeOptions {
                    mode: flowy_ui::ThemeMode::Dark,
                    ..flowy_ui::ThemeOptions::default()
                },
                false,
                cx,
            );
        });
    });
    cx.update(|cx| {
        assert!(first.read(cx).theme.dark);
        assert!(!second.read(cx).theme.dark);
        assert_eq!(first.read(cx).prompt.read(cx).value(), "preserve draft");
    });
    drop(first);
    drop(second);
    drop(theme);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn catalog_loading_holds_queued_receipts_and_queues_new_prompts(
    cx: &mut TestAppContext,
) {
    let root = std::env::temp_dir().join(format!(
        "flowy-studio-catalog-gate-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    flowy_agent::AcceptanceQueue::new(root.clone())
        .accept(None, "persisted before restart", 1)
        .unwrap();
    let panel = open_panel(cx, |window, cx| {
        let ui = UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        StudioPanel::new(
            StudioConfig {
                product: "Flowy",
                i18n: I18n::embedded("Flowy"),
                model: Model::Lite,
                plan: Plan::Free,
                model_info_version: String::new(),
                model_info_open: false,
                store_root: root.clone(),
                llm: LlmConfig {
                    api_key: None,
                    base_url: String::new(),
                    model: String::new(),
                    provider: "flowy".into(),
                    ..LlmConfig::default()
                },
            },
            ui,
            window,
            cx,
        )
    });
    panel.update(cx, |panel, cx| {
        panel.catalog_loading = true;
        panel.cloud_ready = false;
        panel.process_agent_updates(
            vec![flowy_agent::AgentUpdate::Initialized],
            cx,
        );
        assert_eq!(
            panel.pending_receipts.len(),
            1,
            "startup before catalog arrives must not drain into echo"
        );
        assert!(!panel.agent.has_active_claim());

        // Typing and sending while catalog is loading accepts into the
        // durable queue instead of failing with notice.agent_failed.
        panel.fill_prompt("typed while catalog loads", cx);
        panel.send_prompt(cx);
        assert_eq!(panel.notice, None);
        assert_eq!(panel.pending_receipts.len(), 2);
    });
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn switching_to_home_neither_settles_a_running_claim_nor_binds_home(
    cx: &mut TestAppContext,
) {
    let (panel, root) = submission_panel(cx, "claim-isolation");
    panel.update(cx, |panel, cx| {
        let hinted = panel.agent.create_home_session().unwrap();
        panel.open_session(&hinted, cx);
        panel
            .agent
            .accept_deferred("finish in hinted session")
            .unwrap();

        // User goes back to a blank Home screen before the receipt drains.
        panel.new_task(cx);
        assert_eq!(panel.surface, StudioSurface::Home);
        assert_eq!(panel.agent.active_session_id(), None);

        panel.initialized = true;
        panel.refresh_pending_receipts(cx);
        assert!(panel.agent.has_active_claim());
        assert_eq!(
            panel.agent.active_session_id(),
            None,
            "background drain must not bind a blank Home screen to the old session"
        );

        // Wait for the background turn to finish, then verify the claim settles.
        for _ in 0..200 {
            if panel
                .agent
                .claimed_session_status()
                .is_none_or(|status| !status.is_busy())
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panel.refresh_pending_receipts(cx);
        assert!(!panel.agent.has_active_claim());
        assert_eq!(panel.pending_receipts.len(), 0);
    });
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn override_delivery_writes_audit_chain_and_persists_badge_across_reopen(
    cx: &mut TestAppContext,
) {
    let (panel, root) = submission_panel(cx, "override-persist");
    let (session_id, report_id) = panel.update(cx, |panel, cx| {
        let session_id = panel.agent.create_home_session().unwrap();
        panel.open_session(&session_id, cx);
        let workspace =
            root.join("sessions").join("workspaces").join(&session_id);
        let output_dir = workspace.join("outputs/submission-override-test");
        std::fs::create_dir_all(&output_dir).unwrap();
        let deck = output_dir.join("deck.pptx");
        std::fs::write(
            &deck,
            include_bytes!("../../examples/fixtures/preview/sample.pptx"),
        )
        .unwrap();

        let gate =
            flowy_agent::delivery::DeliveryStore::new(root.join("delivery"));
        let report = gate
            .record_report(
                &deck,
                vec![flowy_agent::delivery::Issue {
                    code: "VALUE_MISMATCH".into(),
                    field: "S0-title".into(),
                }],
                100,
            )
            .unwrap();
        assert!(!report.ok);

        panel.confirm_override_for_session(
            &session_id,
            &report.id,
            "outputs/submission-override-test/deck.pptx",
            cx,
        );
        (session_id, report.id)
    });
    let _ = report_id;
    cx.run_until_parked();
    let artifact_id = panel.read_with(cx, |panel, _| {
        assert_eq!(panel.notice, None);
        panel
            .agent
            .chat_history()
            .rows
            .iter()
            .find_map(|line| {
                if let flowy_agent::ChatLine::Artifact { artifact_id } = line {
                    Some(artifact_id.clone())
                } else {
                    None
                }
            })
            .expect("override delivery appends artifact card")
    });

    let events =
        std::fs::read_to_string(root.join("delivery/events.jsonl")).unwrap();
    let blocked_pos = events.find("deliver_blocked").expect("blocked event");
    let override_pos = events.find("deliver_override").expect("override event");
    let deliver_pos = events.find("\"deliver\"").expect("deliver event");
    assert!(blocked_pos < override_pos && override_pos < deliver_pos);

    drop(panel);

    // Reopen over the same root and confirm both the artifact record and
    // the override marker survive restart.
    let reopened = open_panel(cx, |window, cx| {
        let ui = UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        StudioPanel::new(
            StudioConfig {
                product: "Flowy",
                i18n: I18n::embedded("Flowy"),
                model: Model::Lite,
                plan: Plan::Free,
                model_info_version: String::new(),
                model_info_open: false,
                store_root: root.clone(),
                llm: LlmConfig::default(),
            },
            ui,
            window,
            cx,
        )
    });
    reopened.update(cx, |panel, cx| {
        panel.open_session(&session_id, cx);
        let store = panel.agent.artifact_store().unwrap();
        let record = store.get(&artifact_id).unwrap().unwrap();
        assert!(
            panel
                .agent
                .artifact_was_overridden(&record.manifest.receipt.sha256),
            "permanent override mark must survive panel reopen"
        );
        assert!(panel.agent.chat_history().rows.iter().any(|line| {
            matches!(line, flowy_agent::ChatLine::Artifact { artifact_id: id } if id == &artifact_id)
        }));
    });
    drop(reopened);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn dead_receipt_lifecycle_surfaces_notice_and_syncs_visible_receipts(
    cx: &mut TestAppContext,
) {
    let (panel, root) = submission_panel(cx, "dead-receipt-state");
    panel.update(cx, |panel, cx| {
        panel.process_agent_updates(
            vec![flowy_agent::AgentUpdate::Receipt {
                id: "rcpt-dead-1".into(),
                state: ReceiptState::Dead,
            }],
            cx,
        );
        assert_eq!(panel.notice, Some("notice.receipt_gave_up"));
        assert_eq!(panel.visible_receipts, panel.agent.visible_acceptances());

        panel.process_agent_updates(
            vec![flowy_agent::AgentUpdate::Receipt {
                id: "rcpt-q-1".into(),
                state: ReceiptState::Quarantined,
            }],
            cx,
        );
        assert_eq!(panel.notice, Some("notice.receipt_quarantined"));
    });
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn autonomous_drain_apply_config_and_notify_cloud_ready_syncs_queue(
    cx: &mut TestAppContext,
) {
    let root = std::env::temp_dir().join(format!(
        "flowy-studio-auto-cloud-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    let panel = open_panel(cx, |window, cx| {
        let ui = UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        StudioPanel::new(
            StudioConfig {
                product: "Flowy",
                i18n: I18n::embedded("Flowy"),
                model: Model::Lite,
                plan: Plan::Free,
                model_info_version: String::new(),
                model_info_open: false,
                store_root: root.clone(),
                llm: LlmConfig {
                    api_key: None,
                    base_url: String::new(),
                    model: String::new(),
                    provider: "flowy".into(),
                    ..LlmConfig::default()
                },
            },
            ui,
            window,
            cx,
        )
    });
    panel.update(cx, |panel, cx| {
        panel.agent.set_autonomous_drain(true);
        let receipt = panel
            .agent
            .accept_deferred("wake after cloud ready")
            .expect("accepted");
        assert_eq!(panel.agent.pending_acceptances().len(), 1);
        // Kernel 65772f857 wakes autonomous drain inside `apply_config`
        // once the config becomes runnable; `notify_cloud_ready` only
        // syncs visible receipts and rebuilds the board snapshot.
        panel
            .agent
            .apply_config(LlmConfig {
                api_key: None,
                base_url: String::new(),
                model: String::new(),
                provider: String::new(),
                ..LlmConfig::default()
            })
            .expect("apply runnable config");
        panel.notify_cloud_ready(cx);
        assert!(panel.agent.autonomous_drain());
        let _ = receipt;
    });
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn prepare_for_sign_out_cancels_queued_receipts_and_invalidates_submission(
    cx: &mut TestAppContext,
) {
    let (panel, root) = submission_panel(cx, "signout-cancel");
    let stop = Arc::new(AtomicBool::new(false));
    let stop_check = stop.clone();
    panel.update(cx, |panel, cx| {
        let receipt = panel
            .agent
            .accept_deferred("queued before sign-out")
            .expect("accept deferred");
        panel.sync_receipts();
        assert_eq!(panel.visible_receipts.len(), 1);
        panel.submission_pending = true;
        panel.submission = Some(42);
        panel.submission_stop = Some(stop);
        panel.retrying_receipts.insert(receipt.id);
        panel.cached_trace = Some(("s1".into(), Vec::new()));

        panel.prepare_for_sign_out(
            LlmConfig {
                api_key: None,
                base_url: String::new(),
                model: String::new(),
                provider: "flowy".into(),
                ..LlmConfig::default()
            },
            cx,
        );

        assert!(stop_check.load(Ordering::Acquire));
        assert!(!panel.submission_pending);
        assert!(panel.submission.is_none());
        assert!(!panel.cloud_ready);
        assert!(!panel.catalog_loading);
        assert!(!panel.can_submit_prompt());
        assert_eq!(panel.retrying_receipts.len(), 0);
        assert!(panel.cached_trace.is_none());
        assert_eq!(panel.visible_receipts.len(), 0);
        assert_eq!(panel.agent.pending_acceptances().len(), 0);
    });
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn bind_default_workspace_preserves_prompt_draft(cx: &mut TestAppContext) {
    let (panel, root) = submission_panel(cx, "bind-default-workspace");
    let project_dir = root.join("alpha-project");
    std::fs::create_dir_all(&project_dir).unwrap();
    panel.update(cx, |panel, cx| {
        panel
            .create_project("Alpha", &project_dir, cx)
            .expect("create project");
        assert!(panel.active_project_id.is_some());

        panel.fill_prompt("draft must survive workspace unbind", cx);
        panel.bind_default_workspace(cx);

        assert!(panel.active_project_id.is_none());
        assert!(panel.agent.active_workspace().is_none());
        assert_eq!(
            visible_prompt(panel, cx),
            "draft must survive workspace unbind"
        );
    });
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn cloud_ready_gate_blocks_when_offline_and_allows_during_catalog_loading(
    cx: &mut TestAppContext,
) {
    let (panel, root) = submission_panel(cx, "cloud-gate");
    panel.update(cx, |panel, _cx| {
        panel
            .agent
            .apply_config(LlmConfig {
                api_key: None,
                base_url: String::new(),
                model: String::new(),
                provider: "flowy".into(),
                ..LlmConfig::default()
            })
            .unwrap();
        panel.cloud_ready = false;
        panel.catalog_loading = false;
        assert!(!panel.can_submit_prompt());

        panel.catalog_loading = true;
        assert!(panel.can_submit_prompt());

        panel.catalog_loading = false;
        panel.cloud_ready = true;
        assert!(panel.can_submit_prompt());
    });
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn active_popover_enforces_mutual_exclusion_and_overlay_dismissal(
    cx: &mut TestAppContext,
) {
    let (panel, root) = submission_panel(cx, "popover-seam");
    panel.update(cx, |panel, cx| {
        assert_eq!(panel.active_popup_scope(), None);

        panel.toggle_popover(ActivePopover::EnvStatus);
        assert_eq!(panel.active_popup_scope(), Some("env-status-popover"));

        // Opening the unified `+` menu automatically replaces EnvStatus.
        panel.toggle_popover(ActivePopover::AddMenu);
        assert_eq!(panel.active_popup_scope(), Some("composer-add-menu"));
        assert!(panel.is_add_menu_open());

        // Opening the composer model menu replaces AddMenu (never two popovers).
        panel.toggle_popover(ActivePopover::ModelMenu);
        assert_eq!(panel.active_popup_scope(), Some("model-list"));
        assert!(panel.is_model_menu_open());
        assert!(!panel.is_add_menu_open());

        // Same popover toggles closed.
        panel.toggle_popover(ActivePopover::ModelMenu);
        assert_eq!(panel.active_popup_scope(), None);

        // Overlay dismissal only closes matching scope and also handles Flow Modal.
        panel.toggle_popover(ActivePopover::BgTasks);
        assert!(!panel.dismiss_overlay("model-list", cx));
        assert_eq!(panel.active_popup_scope(), Some("bg-tasks-popover"));
        assert!(panel.dismiss_overlay("bg-tasks-popover", cx));
        assert_eq!(panel.active_popup_scope(), None);

        // Opening a missing session must not leave a ghost inspector/drawer.
        panel.select_board_session("s-missing".into(), None, true, cx);
        assert_eq!(panel.board_inspector.session, None);
        assert!(!panel.has_board_drawer());

        // A resolvable session selects cleanly and narrow drawer dismisses via overlay scope.
        panel.surface = StudioSurface::Board;
        panel.board_width_px = 680.0;
        panel
            .board_snapshot
            .sessions
            .push(crate::ports::SessionBoardGroup {
                session_id: "s-demo".into(),
                title: "Demo".into(),
                project_id: None,
                project_name: None,
                workspace_display: String::new(),
                updated_at: 0,
                pinned: false,
                archived: false,
                preview: None,
                assistant_summary: None,
                branches: vec![crate::ports::WorkBranchSummary {
                    id: "b-demo".into(),
                    owner_session_id: None,
                    skill_label: crate::ports::SkillLabel::Key(
                        "board.skill_work",
                    ),
                    phase: crate::ports::BranchPhase::Idle,
                    deliverable_name: "Demo".into(),
                    deliverable_path: None,
                    format: crate::ports::DeliverableFormat::Other,
                    is_workspace_change: false,
                    progress_pct: 100.0,
                    detail: String::new(),
                    work_progress: None,
                }],
                workspace_changes: Vec::new(),
                child_workers: Vec::new(),
                total_branch_count: 1,
                total_file_count: 0,
                session_contract: None,
                has_blocker: false,
                has_delivered: false,
                is_running: false,
                is_active: false,
                group_kpi: crate::ports::BoardKpiSummary::default(),
            });
        panel.select_board_session("s-demo".into(), None, true, cx);
        assert_eq!(panel.board_inspector.session.as_deref(), Some("s-demo"));
        assert!(panel.has_board_drawer());
        assert!(panel.dismiss_overlay("board-inspector-drawer", cx));
        assert!(!panel.has_board_drawer());
    });
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn format_skill_prompt_uses_real_slash_skill_protocol() {
    let chip = crate::ports::SkillChip {
        id: "flowy/xlsx-summary".into(),
        title: "Excel Summary".into(),
        summary: String::new(),
        official: true,
    };
    assert_eq!(
        format_skill_prompt(Some(&chip), "summarize Q3.xlsx"),
        "/skill:flowy/xlsx-summary summarize Q3.xlsx"
    );
    // Never double-prefix if the user already typed `/skill:...`.
    assert_eq!(
        format_skill_prompt(Some(&chip), "/skill:other/foo run"),
        "/skill:other/foo run"
    );
    assert_eq!(format_skill_prompt(Some(&chip), ""), "");
    assert_eq!(format_skill_prompt(None, "plain prompt"), "plain prompt");
}

#[gpui::test]
fn pr2b_entry_three_paths_create_zero_empty_sessions_and_inherit_project_or_unbind(
    cx: &mut TestAppContext,
) {
    let (panel, root) = submission_panel(cx, "pr2b-entry-zero");
    let dir_a = root.join("proj-alpha");
    std::fs::create_dir_all(&dir_a).unwrap();
    panel.update(cx, |panel, cx| {
        // Home -> repeated `new_task` creates 0 sessions.
        panel.new_task(cx);
        panel.new_task(cx);
        assert_eq!(panel.agent.list_sessions().len(), 0);
        assert!(panel.active_project_id.is_none());

        // Create & open Project A -> `new_task` inherits Project A without creating an empty session.
        let proj_a = panel
            .create_project("Alpha", &dir_a, cx)
            .expect("create project A");
        assert_eq!(
            panel.active_project_id.as_deref(),
            Some(proj_a.id.as_str())
        );
        panel.new_task(cx);
        assert_eq!(
            panel.active_project_id.as_deref(),
            Some(proj_a.id.as_str()),
            "new_task from a Project view must inherit active_project_id"
        );
        assert_eq!(
            panel.agent.active_workspace().map(|w| w.project_id),
            Some(proj_a.id.clone())
        );
        assert_eq!(panel.agent.list_sessions().len(), 0);

        // Project capsule `×` (`bind_default_workspace`) unbinds ONLY the draft, keeping text!
        panel.fill_prompt("draft survives project unbind", cx);
        panel.bind_default_workspace(cx);
        assert!(panel.active_project_id.is_none());
        assert!(panel.agent.active_workspace().is_none());
        assert_eq!(visible_prompt(panel, cx), "draft survives project unbind");
        assert_eq!(panel.agent.list_sessions().len(), 0);

        // Opening Chief Assistant from a project returns cleanly to Home with 0 empty sessions.
        panel.open_project(&proj_a.id, cx).unwrap();
        panel.open_chief_assistant(cx);
        assert!(panel.active_project_id.is_none());
        assert!(panel.agent.active_workspace().is_none());
        assert_eq!(panel.agent.list_sessions().len(), 0);
    });
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn pr2b_first_send_project_a_then_switch_to_project_b_preserves_b_draft_and_routes_to_a(
    cx: &mut TestAppContext,
) {
    let (panel, root) = submission_panel(cx, "pr2b-switch-ab");
    let dir_a = root.join("proj-a");
    let dir_b = root.join("proj-b");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();
    let (proj_a_id, proj_b_id) = panel.update(cx, |panel, cx| {
        let a = panel.create_project("ProjA", &dir_a, cx).unwrap();
        let b = panel.create_project("ProjB", &dir_b, cx).unwrap();
        panel.open_project(&a.id, cx).unwrap();
        panel.fill_prompt("message strictly for A", cx);
        panel.send_prompt(cx);
        assert!(panel.submission_pending);

        // Mid-send switch to Project B and type a new draft for B.
        panel.open_project(&b.id, cx).unwrap();
        panel.fill_prompt("unsent draft for B", cx);
        (a.id, b.id)
    });
    cx.run_until_parked();
    cx.read_entity(&panel, |panel, cx| {
        // Project B stays selected and its draft is completely untouched.
        assert_eq!(
            panel.active_project_id.as_deref(),
            Some(proj_b_id.as_str())
        );
        assert_eq!(visible_prompt(panel, cx), "unsent draft for B");
        // Exactly 1 Router session exists, belonging strictly to Project A (0 for B).
        let sessions = panel.agent.list_sessions();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].project_id.as_deref(), Some(proj_a_id.as_str()));
        assert_eq!(
            panel.agent.session_role(&sessions[0].id),
            flowy_agent::SessionRole::Router
        );
    });
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn pr2b_legacy_standard_session_and_router_stop_and_locate_source_coexist(
    cx: &mut TestAppContext,
) {
    let (panel, root) = submission_panel(cx, "pr2b-legacy-compat");
    let standard_id = panel.update(cx, |panel, cx| {
        let sid = panel.agent.create_home_session().unwrap();
        assert_eq!(
            panel.agent.session_role(&sid),
            flowy_agent::SessionRole::Standard
        );
        panel.open_session(&sid, cx);
        panel.fill_prompt("legacy standard prompt", cx);
        panel.send_prompt(cx);
        sid
    });
    cx.run_until_parked();
    panel.update(cx, |panel, cx| {
        assert_eq!(
            panel.agent.active_session_id().as_deref(),
            Some(standard_id.as_str())
        );
        assert_eq!(
            panel.agent.session_role(&standard_id),
            flowy_agent::SessionRole::Standard
        );
        assert!(panel.agent.has_messages());
        panel.stop_turn(cx);

        // Router session uses Router role and supports child card error
        // surfacing on parent Router (`resolve_pending_card`).
        let router_id = panel
            .agent
            .create_router_session()
            .expect("create router session");
        assert_eq!(
            panel.agent.session_role(&router_id),
            flowy_agent::SessionRole::Router
        );
        panel.open_session(&router_id, cx);
        panel.resolve_pending_card(
            "w-missing-child".into(),
            router_id.clone(),
            "card-missing".into(),
            flowy_agent::CardSelection::ApproveTool {
                args_hash: "deadbeef".into(),
            },
            cx,
        );
    });
    cx.run_until_parked();
    cx.read_entity(&panel, |panel, _| {
        assert!(
            panel.notice.is_some(),
            "child Worker card resolution failure must surface notice on parent Router"
        );
    });
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn pr2b_cached_popover_and_home_pill_unify_child_work_attention_counts(
    cx: &mut TestAppContext,
) {
    let (panel, root) = submission_panel(cx, "pr2b-cached-pill");
    panel.update(cx, |panel, cx| {
        // Seed cached popover snapshot with an awaiting_approval child work
        // in another Router conversation and verify Home + Chat pills and
        // `background_work_count` share one exact count and Attention tone.
        panel.popover_works = crate::router_works::PopoverWorksSnapshot {
            current_works: Vec::new(),
            other_works: vec![(
                "Parent Router".into(),
                flowy_agent::ChildWorkSummary {
                    work_id: "w-attn-1".into(),
                    root_session_id: "r-parent-1".into(),
                    title: "Dangerous shell step".into(),
                    state: "awaiting_approval".into(),
                    access_mode: flowy_agent::WorkAccessMode::ReadWrite,
                    origin: flowy_agent::SessionOrigin::Home,
                    workspace_root: None,
                    turn_status: flowy_agent::TurnStatus::WaitingForUser,
                    updated_at: 1_760_000_000_000,
                    contract_revision: None,
                    delivered_artifact_ids: Vec::new(),
                    missing_items: Vec::new(),
                    objects: Vec::new(),
                    last_stop_handoff: None,
                    heartbeat: None,
                    pending_cards: Vec::new(),
                },
            )],
            legacy_running_sessions: Vec::new(),
            legacy_receipts: Vec::new(),
        };
        panel.surface = StudioSurface::Home;
        assert_eq!(panel.background_work_count(), 1);
        assert_eq!(panel.popover_total_count(), 1);
        let pill = panel
            .router_bg_pill_summary()
            .expect("Home pill must surface awaiting_approval child work");
        assert_eq!(pill.count, 1);
        assert_eq!(pill.tone, crate::router_works::BgPillTone::Attention);
        assert_eq!(
            pill.label,
            panel.tf("chat.bg_tasks_needs_approval", &[("count", "1")])
        );

        // Opening Flowchart Modal on a child Worker branch without a report
        // still resolves cleanly and never leaves a dangling state.
        let router_id =
            panel.agent.create_router_session().expect("create router");
        panel.stop_session_turn(&router_id, cx);
    });
    cx.run_until_parked();
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

fn open_cold_panel(
    cx: &mut TestAppContext,
    tag: &str,
) -> (Entity<StudioPanel>, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "flowy-studio-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let panel = open_panel(cx, |window, cx| {
        let ui = UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        StudioPanel::new(
            StudioConfig {
                product: "Flowy",
                i18n: I18n::embedded("Flowy"),
                model: Model::Lite,
                plan: Plan::Free,
                model_info_version: String::new(),
                model_info_open: false,
                store_root: root.clone(),
                llm: LlmConfig {
                    api_key: None,
                    base_url: String::new(),
                    model: String::new(),
                    provider: "flowy".into(),
                    ..LlmConfig::default()
                },
            },
            ui,
            window,
            cx,
        )
    });
    panel.update(cx, |panel, _| {
        while panel.agent.is_initializing() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    });
    (panel, root)
}

#[gpui::test]
fn cold_start_project_a_draft_switch_to_b_creates_single_project_a_router_and_preserves_b_draft(
    cx: &mut TestAppContext,
) {
    let (panel, root) = open_cold_panel(cx, "cold-proj");
    let proj_a_dir = root.join("proj-a");
    let proj_b_dir = root.join("proj-b");
    std::fs::create_dir_all(&proj_a_dir).unwrap();
    std::fs::create_dir_all(&proj_b_dir).unwrap();
    let proj_a_id = Rc::new(RefCell::new(String::new()));
    let proj_b_id = Rc::new(RefCell::new(String::new()));
    panel.update(cx, |panel, cx| {
        panel
            .create_project("Alpha", &proj_a_dir, cx)
            .expect("create project A");
        *proj_a_id.borrow_mut() =
            panel.active_project_id.clone().expect("A active");
        panel
            .create_project("Beta", &proj_b_dir, cx)
            .expect("create project B");
        *proj_b_id.borrow_mut() =
            panel.active_project_id.clone().expect("B active");

        panel
            .open_project(&proj_a_id.borrow(), cx)
            .expect("open project A draft");
        panel.catalog_loading = true;
        panel.cloud_ready = false;
        panel.initialized = true;
        panel.fill_prompt("first cold task for project A", cx);
        panel.send_prompt(cx);
        assert_eq!(panel.notice, None);
        assert_eq!(visible_prompt(panel, cx), "");
        assert_eq!(panel.pending_receipts.len(), 1);
        let pending = panel.agent.pending_acceptances();
        assert_eq!(pending.len(), 1);
        let draft = pending[0]
            .draft_target
            .as_deref()
            .expect("cold-start receipt carries draft_target");
        assert_eq!(draft.role, flowy_agent::SessionRole::Router);
        assert_eq!(
            draft.origin,
            flowy_agent::SessionOrigin::Project {
                project_id: Some(proj_a_id.borrow().clone()),
            }
        );

        // Switch to Project B draft before catalog finishes loading.
        panel
            .open_project(&proj_b_id.borrow(), cx)
            .expect("open project B draft");
        panel.fill_prompt("keep project B draft", cx);
        assert_eq!(
            panel.active_project_id.as_deref(),
            Some(proj_b_id.borrow().as_str())
        );

        // Model catalog finishes loading with local echo runner enabled:
        // drain the queued Project A receipt without hijacking Project B.
        panel.agent.apply_config(LlmConfig::default()).unwrap();
        panel.catalog_loading = false;
        panel.cloud_ready = true;
        panel.notify_cloud_ready(cx);
        panel.refresh_pending_receipts(cx);
    });
    cx.run_until_parked();
    panel.update(cx, |panel, cx| {
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(3);
        while panel.agent.has_active_claim()
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(10));
            panel.refresh_pending_receipts(cx);
        }
        let sessions = panel.agent.list_sessions();
        assert_eq!(
            sessions.len(),
            1,
            "cold-start Project A drain must create strictly one Router session"
        );
        assert_eq!(
            panel.agent.session_role(&sessions[0].id),
            flowy_agent::SessionRole::Router
        );
        assert_eq!(
            sessions[0].project_id.as_deref(),
            Some(proj_a_id.borrow().as_str())
        );
        assert_eq!(
            panel.active_project_id.as_deref(),
            Some(proj_b_id.borrow().as_str()),
            "Project B selection must stay intact"
        );
        assert_eq!(
            panel.agent.active_session_id(),
            None,
            "background drain must not steal Project B draft focus"
        );
        assert_eq!(visible_prompt(panel, cx), "keep project B draft");
    });
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn cold_start_existing_router_queues_instead_of_rejecting_library_loading(
    cx: &mut TestAppContext,
) {
    let (panel, root) = open_cold_panel(cx, "cold-router");
    let proj_a_dir = root.join("proj-a");
    std::fs::create_dir_all(&proj_a_dir).unwrap();
    let router_id = Rc::new(RefCell::new(String::new()));
    panel.update(cx, |panel, cx| {
        let rid = panel
            .agent
            .create_router_project_session(flowy_agent::AgentWorkspace {
                project_id: "proj-a".into(),
                root: proj_a_dir.clone(),
            })
            .expect("create router");
        *router_id.borrow_mut() = rid.clone();
        panel.open_session(&rid, cx);
        panel.catalog_loading = true;
        panel.cloud_ready = false;
        panel.initialized = true;

        panel.fill_prompt("existing router message while catalog loads", cx);
        panel.send_prompt(cx);
        assert_eq!(
            panel.notice, None,
            "existing Router must accept durably instead of rejecting library.loading"
        );
        assert_eq!(visible_prompt(panel, cx), "");
        let pending = panel.agent.pending_acceptances();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].strict_target);
        assert_eq!(pending[0].target_session_id.as_deref(), Some(rid.as_str()));

        panel.agent.apply_config(LlmConfig::default()).unwrap();
        panel.catalog_loading = false;
        panel.cloud_ready = true;
        panel.notify_cloud_ready(cx);
        panel.refresh_pending_receipts(cx);
    });
    cx.run_until_parked();
    panel.update(cx, |panel, cx| {
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(3);
        while panel.agent.has_active_claim()
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(10));
            panel.refresh_pending_receipts(cx);
        }
        assert_eq!(panel.agent.list_sessions().len(), 1);
        let lines = panel.agent.session_lines(&router_id.borrow());
        assert!(lines.iter().any(|l| matches!(
            l,
            flowy_agent::ChatLine::User { text }
                if text == "existing router message while catalog loads"
        )));
    });
    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn quote_banner_overwrites_clears_isolates_sessions_and_roundtrips_skill_prompt(
    cx: &mut TestAppContext,
) {
    let root = std::env::temp_dir().join(format!(
        "flowy-studio-quote-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    let (panel, cx) = cx.add_window_view(|window, cx| {
        let ui = UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        let mut panel = StudioPanel::new(
            StudioConfig {
                product: "Flowy",
                i18n: I18n::embedded("Flowy"),
                model: Model::Lite,
                plan: Plan::Free,
                model_info_version: String::new(),
                model_info_open: false,
                store_root: root.clone(),
                llm: LlmConfig {
                    api_key: None,
                    ..LlmConfig::default()
                },
            },
            ui,
            window,
            cx,
        );
        panel.catalog.replace(vec![flowy_agent::CatalogModel {
            id: "test-model".into(),
            name: "Test model".into(),
            ..Default::default()
        }]);
        panel.cloud_ready = true;
        panel
    });
    let sid_a = Rc::new(RefCell::new(String::new()));
    let sid_b = Rc::new(RefCell::new(String::new()));
    panel.update(cx, |panel, cx| {
        let a = panel
            .agent
            .create_router_session()
            .expect("create router session A");
        let b = panel
            .agent
            .create_router_session()
            .expect("create router session B");
        *sid_a.borrow_mut() = a.clone();
        *sid_b.borrow_mut() = b;
        panel.open_session(&a, cx);

        // Second quote replaces first quote cleanly (single-quote WeChat semantics).
        panel.set_active_quote(
            crate::composer::QuoteRole::Assistant,
            "第一条长引用：先查环境诊断",
            cx,
        );
        panel.set_active_quote(
            crate::composer::QuoteRole::User,
            &"第二条超长中文引用摘要：".repeat(16),
            cx,
        );
        let quote = panel.active_quote.as_ref().expect("latest quote kept");
        assert_eq!(quote.role, crate::composer::QuoteRole::User);
        assert!(quote.excerpt.chars().count() <= 160);
        assert!(quote.excerpt.ends_with('\u{2026}'));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("composer-quote-banner").is_some());

    // Clicking X clears the quote banner immediately.
    let clear_btn = cx
        .debug_bounds("composer-quote-clear")
        .expect("quote clear X button rendered");
    cx.simulate_click(clear_btn.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.read_entity(&panel, |panel, _| {
        assert!(panel.active_quote.is_none());
    });
    assert!(cx.debug_bounds("composer-quote-banner").is_none());

    // Switching sessions isolates quotes per conversation.
    panel.update(cx, |panel, cx| {
        panel.set_active_quote(
            crate::composer::QuoteRole::Assistant,
            "会话 A 的关键结论",
            cx,
        );
        panel.open_session(&sid_b.borrow(), cx);
        assert!(
            panel.active_quote.is_none(),
            "session B must not inherit session A quote"
        );
        panel.open_session(&sid_a.borrow(), cx);
        assert_eq!(
            panel.active_quote.as_ref().map(|q| q.excerpt.as_str()),
            Some("会话 A 的关键结论")
        );
    });

    // Cold-start deferred send with mounted `/skill:` preserves `/skill:` prefix & quote cleanly.
    panel.update(cx, |panel, cx| {
        panel.selected_skill = Some(crate::ports::SkillChip {
            id: "flowy/xlsx-summary".into(),
            title: "表格总结".into(),
            summary: String::new(),
            official: true,
        });
        panel.catalog_loading = true;
        panel.cloud_ready = false;
        panel.fill_prompt("整理本季度销售报表", cx);
        panel.send_prompt(cx);
        assert!(
            panel.active_quote.is_none(),
            "quote clears once deferred prompt is accepted"
        );
        let pending = panel.agent.pending_acceptances();
        assert_eq!(pending.len(), 1);
        assert!(
            pending[0]
                .text
                .starts_with("/skill:flowy/xlsx-summary > \u{275d} Flowy: 会话 A 的关键结论\n\n整理本季度销售报表"),
            "packed deferred text was: {}",
            pending[0].text
        );

        // Render a User bubble containing that packed text and confirm top quote preview appears.
        panel.feed.update(cx, |feed, cx| {
            feed.sync_history(
                Some(sid_a.borrow().clone()),
                flowy_agent::ChatHistory {
                    rows: vec![flowy_agent::ChatLine::User {
                        text: pending[0].text.clone(),
                    }],
                    streaming: None,
                    status: flowy_agent::TurnStatus::Completed,
                },
                cx,
            );
        });
        panel.surface = StudioSurface::Chat;
        cx.notify();
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("chat-user-quote-preview").is_some(),
        "quoted User bubble renders top excerpt strip"
    );

    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn quote_session_sync_covers_home_project_and_artifact_transitions(
    cx: &mut TestAppContext,
) {
    let root = std::env::temp_dir().join(format!(
        "flowy_studio_quote_sync_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    let proj_dir = root.join("proj");
    std::fs::create_dir_all(&proj_dir).expect("create project folder");
    let (panel, cx) = cx.add_window_view(|window, cx| {
        let ui = UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        StudioPanel::new(
            StudioConfig {
                product: "Flowy",
                i18n: I18n::embedded("Flowy"),
                model: Model::Lite,
                plan: Plan::Free,
                model_info_version: String::new(),
                model_info_open: false,
                store_root: root.clone(),
                llm: LlmConfig {
                    api_key: None,
                    ..LlmConfig::default()
                },
            },
            ui,
            window,
            cx,
        )
    });
    let sid_a = Rc::new(RefCell::new(String::new()));
    let sid_b = Rc::new(RefCell::new(String::new()));
    panel.update(cx, |panel, cx| {
        let a = panel
            .agent
            .create_router_session()
            .expect("create session A");
        let b = panel
            .agent
            .create_router_session()
            .expect("create session B");
        *sid_a.borrow_mut() = a.clone();
        *sid_b.borrow_mut() = b.clone();
        panel.open_session(&a, cx);
        panel.set_active_quote(
            crate::composer::QuoteRole::Assistant,
            "会话 A 的待保留引用",
            cx,
        );

        // External session clear followed by show_new_home stashes A's quote cleanly.
        let _ = panel.agent.clear_active();
        panel.show_new_home(cx);
        assert!(
            panel.active_quote.is_none(),
            "home view clears active quote banner"
        );
        panel.open_session(&a, cx);
        assert_eq!(
            panel.active_quote.as_ref().map(|q| q.excerpt.as_str()),
            Some("会话 A 的待保留引用"),
            "reopening A restores stashed quote after show_new_home"
        );

        // Host selecting B then calling show_selected_artifact syncs active_quote via refresh_chat.
        panel.agent.select_session(&b).expect("host select B");
        let _ = panel.show_selected_artifact("missing-artifact", cx);
        assert!(
            panel.active_quote.is_none(),
            "show_selected_artifact drops A's quote when agent active session is B"
        );
        panel.open_session(&a, cx);
        assert_eq!(
            panel.active_quote.as_ref().map(|q| q.excerpt.as_str()),
            Some("会话 A 的待保留引用"),
            "returning to A after show_selected_artifact still preserves A's quote"
        );

        // Rolled-back open_artifact_source keeps A's quote intact.
        assert!(panel
            .open_artifact_source(&b, "missing-artifact", cx)
            .is_err());
        assert_eq!(
            panel.active_quote.as_ref().map(|q| q.excerpt.as_str()),
            Some("会话 A 的待保留引用"),
            "failed open_artifact_source rollback restores A's quote"
        );

        // Opening a project stashes A's quote cleanly without double-stashing.
        panel
            .create_project("Quote Proj", &proj_dir, cx)
            .expect("create and open project");
        assert!(panel.active_quote.is_none());
        panel.open_session(&a, cx);
        assert_eq!(
            panel.active_quote.as_ref().map(|q| q.excerpt.as_str()),
            Some("会话 A 的待保留引用"),
            "open_project must preserve session A draft quote"
        );

        // clear_thread discards the active session's quote permanently.
        panel.clear_thread(cx);
        assert!(panel.active_quote.is_none());
        panel.open_session(&a, cx);
        assert!(
            panel.active_quote.is_none(),
            "clear_thread permanently removes session A quote"
        );
    });

    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn drawer_blocks_overturned_history_clicks_preserves_turn_snapshot_and_evicts_failed_preview(
    cx: &mut TestAppContext,
) {
    let root = std::env::temp_dir().join(format!(
        "flowy_studio_drawer_settlement_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    let work_dir = root.join("work");
    std::fs::create_dir_all(&work_dir).expect("create work dir");
    let a_path = work_dir.join("alpha.md");
    let b_path = work_dir.join("beta.md");
    std::fs::write(&a_path, "alpha preview").expect("write alpha");
    std::fs::write(&b_path, "beta preview").expect("write beta");
    let a_str = a_path.to_string_lossy().into_owned();
    let a_variant =
        format!(r"\\?\{}", a_str.to_ascii_uppercase().replace('\\', "/"));
    let b_str = b_path.to_string_lossy().into_owned();

    let (panel, cx) = cx.add_window_view(|window, cx| {
        let ui = UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        StudioPanel::new(
            StudioConfig {
                product: "Flowy",
                i18n: I18n::embedded("Flowy"),
                model: Model::Lite,
                plan: Plan::Free,
                model_info_version: String::new(),
                model_info_open: false,
                store_root: root.clone(),
                llm: LlmConfig {
                    api_key: None,
                    ..LlmConfig::default()
                },
            },
            ui,
            window,
            cx,
        )
    });

    let entry_a = FileEntry {
        path: a_str.clone(),
        change: flowy_agent::FileChangeKind::Add,
        added: 1,
        removed: 0,
        binary: false,
    };
    let entry_b = FileEntry {
        path: b_str.clone(),
        change: flowy_agent::FileChangeKind::Add,
        added: 1,
        removed: 0,
        binary: false,
    };
    let verify_line =
        |id: &str, path: &str, ok: bool| flowy_agent::ChatLine::Verification {
            report_id: id.into(),
            artifact_path: path.into(),
            artifact_hash: format!("hash-{id}"),
            ok,
            issues: Vec::new(),
            semantic_hint: None,
            override_used: false,
            provenance_warnings: Vec::new(),
            source_session_id: None,
        };

    // 1. User pins Turn 1 snapshot [A]; Turn 2 delivers independent [B];
    //    sync must preserve [A] instead of kicking user to [B].
    panel.update(cx, |panel, cx| {
        let sid = panel.agent.create_router_session().expect("create session");
        panel.open_session(&sid, cx);
        panel.feed.update(cx, |feed, cx| {
            feed.sync_history(
                Some(sid.clone()),
                flowy_agent::ChatHistory {
                    rows: vec![
                        flowy_agent::ChatLine::User {
                            text: "turn 1".into(),
                        },
                        flowy_agent::ChatLine::FileChanges {
                            files: vec![entry_a.clone()],
                        },
                        verify_line("r1", &a_str, true),
                    ],
                    streaming: None,
                    status: flowy_agent::TurnStatus::Completed,
                },
                cx,
            );
            cx.emit(OpenDeliverables(vec![entry_a.clone()]));
        });
        cx.notify();
    });
    cx.run_until_parked();
    panel.update(cx, |panel, cx| {
        assert!(panel.right_drawer_open);
        assert_eq!(
            panel
                .deliverables
                .as_ref()
                .map(crate::deliverables::DeliverableScope::files),
            Some(&[entry_a.clone()][..])
        );

        // Turn 2 adds B while Turn 1 snapshot is pinned open.
        let sid = panel.agent.active_session_id().unwrap();
        panel.feed.update(cx, |feed, cx| {
            feed.sync_history(
                Some(sid),
                flowy_agent::ChatHistory {
                    rows: vec![
                        flowy_agent::ChatLine::User {
                            text: "turn 1".into(),
                        },
                        flowy_agent::ChatLine::FileChanges {
                            files: vec![entry_a.clone()],
                        },
                        verify_line("r1", &a_str, true),
                        flowy_agent::ChatLine::User {
                            text: "turn 2".into(),
                        },
                        flowy_agent::ChatLine::FileChanges {
                            files: vec![entry_b.clone()],
                        },
                        verify_line("r2", &b_str, true),
                    ],
                    streaming: None,
                    status: flowy_agent::TurnStatus::Completed,
                },
                cx,
            );
        });
        panel.sync_right_drawer_deliverables(cx);
        assert_eq!(
            panel.deliverables,
            Some(crate::deliverables::DeliverableScope::TurnSnapshot(vec![
                entry_a.clone(),
            ])),
            "live sync must not overwrite pinned historical turn snapshot"
        );
        assert_eq!(panel.deliverable_count(cx), 1);

        // Start previewing A, then Turn 3 fails verification on A via variant path.
        panel.set_preview_file(Some(a_str.clone()), cx);
        assert_eq!(panel.drawer_preview.file.as_deref(), Some(a_str.as_str()));
    });
    cx.run_until_parked();
    panel.update(cx, |panel, cx| {
        assert!(matches!(
            panel.drawer_preview.body,
            Some(DrawerPreviewBody::Markdown(_))
        ));

        let sid = panel.agent.active_session_id().unwrap();
        panel.feed.update(cx, |feed, cx| {
            feed.sync_history(
                Some(sid),
                flowy_agent::ChatHistory {
                    rows: vec![
                        flowy_agent::ChatLine::User {
                            text: "turn 1".into(),
                        },
                        flowy_agent::ChatLine::FileChanges {
                            files: vec![entry_a.clone()],
                        },
                        verify_line("r1", &a_str, true),
                        flowy_agent::ChatLine::User {
                            text: "turn 2".into(),
                        },
                        flowy_agent::ChatLine::FileChanges {
                            files: vec![entry_b.clone()],
                        },
                        verify_line("r2", &b_str, true),
                        flowy_agent::ChatLine::User {
                            text: "turn 3".into(),
                        },
                        verify_line("r3", &a_variant, false),
                    ],
                    streaming: None,
                    status: flowy_agent::TurnStatus::Completed,
                },
                cx,
            );
        });
        panel.sync_right_drawer_deliverables(cx);
        assert!(
            panel.drawer_preview.file.is_none()
                && panel.drawer_preview.body.is_none(),
            "active preview must be evicted when later turn fails verification"
        );
        assert!(panel.deliverables.is_none());
        assert_eq!(panel.notice, Some("verify.card.failed"));

        // Attempting to re-open historical FileChanges([A]) or preview A is blocked.
        panel.close_right_sheet(cx);
        panel.emit_notice(None, cx);
        panel.open_deliverables(vec![entry_a.clone()], cx);
        assert!(!panel.right_drawer_open);
        assert_eq!(panel.notice, Some("verify.card.failed"));

        panel.emit_notice(None, cx);
        panel.set_preview_file(Some(a_str.clone()), cx);
        assert!(panel.drawer_preview.file.is_none());
        assert_eq!(panel.notice, Some("verify.card.failed"));
    });

    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn drawer_workbench_async_preview_fix_in_chat_and_scope_switching(
    cx: &mut TestAppContext,
) {
    let root = std::env::temp_dir()
        .join(format!("flowy-studio-workbench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let work_dir = root.join("work");
    std::fs::create_dir_all(&work_dir).unwrap();
    let file_a = work_dir.join("alpha.md");
    let file_b = work_dir.join("beta.csv");
    let file_bad = work_dir.join("broken.csv");
    std::fs::write(&file_a, "# Alpha Report\nDone.").unwrap();
    std::fs::write(&file_b, "col,val\nok,1\n").unwrap();
    std::fs::write(&file_bad, "col,val\nbad,0\n").unwrap();
    let a_str = file_a.to_string_lossy().into_owned();
    let b_str = file_b.to_string_lossy().into_owned();
    let bad_str = file_bad.to_string_lossy().into_owned();

    let (panel, cx) = cx.add_window_view(|window, cx| {
        let ui = UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        StudioPanel::new(
            StudioConfig {
                product: "Flowy",
                i18n: I18n::embedded("Flowy"),
                model: Model::Lite,
                plan: Plan::Free,
                model_info_version: String::new(),
                model_info_open: false,
                store_root: root.clone(),
                llm: LlmConfig::default(),
            },
            ui,
            window,
            cx,
        )
    });

    panel.update(cx, |panel, cx| {
        let sid = panel.agent.create_session().unwrap();
        panel.agent.select_session(&sid).unwrap();
        let entry_a = flowy_agent::FileEntry {
            path: a_str.clone(),
            change: flowy_agent::FileChangeKind::Add,
            added: 2,
            removed: 0,
            binary: false,
        };
        let entry_b = flowy_agent::FileEntry {
            path: b_str.clone(),
            change: flowy_agent::FileChangeKind::Add,
            added: 2,
            removed: 0,
            binary: false,
        };
        panel.feed.update(cx, |feed, cx| {
            feed.sync_history(
                Some(sid),
                flowy_agent::ChatHistory {
                    rows: vec![
                        flowy_agent::ChatLine::User {
                            text: "turn 1".into(),
                        },
                        flowy_agent::ChatLine::FileChanges {
                            files: vec![entry_a],
                        },
                        flowy_agent::ChatLine::Verification {
                            report_id: "r1".into(),
                            artifact_path: a_str.clone(),
                            artifact_hash: "11111111".into(),
                            ok: true,
                            issues: Vec::new(),
                            semantic_hint: None,
                            override_used: false,
                            provenance_warnings: Vec::new(),
                            source_session_id: None,
                        },
                        flowy_agent::ChatLine::User {
                            text: "turn 2".into(),
                        },
                        flowy_agent::ChatLine::FileChanges {
                            files: vec![entry_b],
                        },
                        flowy_agent::ChatLine::Verification {
                            report_id: "r-bad".into(),
                            artifact_path: bad_str.clone(),
                            artifact_hash: "22222222".into(),
                            ok: false,
                            issues: vec![flowy_agent::VerificationIssue {
                                code: "formula_error".into(),
                                field: "Sheet1!B2".into(),
                            }],
                            semantic_hint: None,
                            override_used: false,
                            provenance_warnings: Vec::new(),
                            source_session_id: None,
                        },
                    ],
                    streaming: None,
                    status: flowy_agent::TurnStatus::Completed,
                },
                cx,
            );
        });

        // Toggle drawer captures both failed banner & latest workspace items.
        panel.toggle_deliverables(cx);
        assert!(panel.right_drawer_open);
        assert!(panel.feed.read(cx).has_trace_attention());
        assert_eq!(panel.feed.read(cx).failed_verifications().len(), 1);
        assert_eq!(
            panel.visible_workspace_deliverables(cx).len(),
            1,
            "Latest scope shows only turn 2 settled beta.csv"
        );

        // Switch to All scope -> surfaces alpha.md + beta.csv.
        panel.set_deliverables_view_scope(
            crate::deliverables::DeliverablesViewScope::All,
            cx,
        );
        assert_eq!(panel.visible_workspace_deliverables(cx).len(), 2);

        // Rapid async preview switch A -> B cancels stale A load.
        panel.set_preview_file(Some(a_str.clone()), cx);
        panel.set_preview_file(Some(b_str.clone()), cx);
        assert_eq!(panel.drawer_preview.file.as_deref(), Some(b_str.as_str()));
        assert!(matches!(
            panel.drawer_preview.body,
            Some(DrawerPreviewBody::Loading)
        ));
    });
    cx.run_until_parked();
    panel.update(cx, |panel, cx| {
        assert_eq!(panel.drawer_preview.file.as_deref(), Some(b_str.as_str()));
        assert!(matches!(
            panel.drawer_preview.body,
            Some(DrawerPreviewBody::Table { .. })
        ));
        assert!(
            !panel.deliverable_sizes.is_empty(),
            "background metadata probe populates cached sizes"
        );

        // Fix in chat fills empty prompt, then preserves non-empty draft as quote.
        panel.request_fix_in_chat(
            "Sheet1!B2".into(),
            Some(bad_str.clone()),
            None,
            cx,
        );
        assert!(
            panel.prompt_text(cx).contains("broken.csv"),
            "empty prompt filled with fix request"
        );
        assert!(panel.active_quote.is_none());

        panel.set_prompt_text("keep my draft", cx);
        panel.request_fix_in_chat("Sheet1!B2".into(), Some(bad_str), None, cx);
        assert_eq!(panel.prompt_text(cx), "keep my draft");
        assert!(
            panel
                .active_quote
                .as_ref()
                .is_some_and(|q| q.excerpt.contains("broken.csv")),
            "existing draft must stay intact while fix context moves to quote"
        );
    });

    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn open_chief_assistant_reset_preserves_collapsed_dock_visible(
    cx: &mut gpui::TestAppContext,
) {
    let root = std::env::temp_dir()
        .join(format!("flowy-panel-dock-reset-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create test root");

    let (panel, cx) = cx.add_window_view(|window, cx| {
        let ui = UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        StudioPanel::new(
            StudioConfig {
                product: "Flowy",
                i18n: I18n::embedded("Flowy"),
                model: Model::Lite,
                plan: Plan::Free,
                model_info_version: String::new(),
                model_info_open: false,
                store_root: root.clone(),
                llm: LlmConfig::default(),
            },
            ui,
            window,
            cx,
        )
    });

    panel.update(cx, |panel, cx| {
        panel.set_dock_visible(false, cx);
        assert!(!panel.dock_visible);
        panel.open_chief_assistant(cx);
        assert!(
            !panel.dock_visible,
            "reset_session from open_chief_assistant must not overwrite collapsed dock_visible"
        );
    });

    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn set_artifacts_with_open_drawer_and_empty_workspace_files_notifies_panel(
    cx: &mut gpui::TestAppContext,
) {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let root = std::env::temp_dir().join(format!(
        "flowy-panel-artifacts-notify-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create test root");

    let (panel, cx) = cx.add_window_view(|window, cx| {
        let ui = UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        StudioPanel::new(
            StudioConfig {
                product: "Flowy",
                i18n: I18n::embedded("Flowy"),
                model: Model::Lite,
                plan: Plan::Free,
                model_info_version: String::new(),
                model_info_open: false,
                store_root: root.clone(),
                llm: LlmConfig::default(),
            },
            ui,
            window,
            cx,
        )
    });

    let notifies = Arc::new(AtomicUsize::new(0));
    let notifies_ref = Arc::clone(&notifies);
    let _sub = cx.update(|_, cx| {
        cx.observe(&panel, move |_, _| {
            notifies_ref.fetch_add(1, Ordering::SeqCst);
        })
    });

    panel.update(cx, |panel, cx| {
        let sid = panel.agent.create_session().expect("create session");
        panel.agent.select_session(&sid).expect("select session");
        panel.feed.update(cx, |feed, cx| {
            feed.sync_history(
                Some(sid),
                flowy_agent::ChatHistory {
                    rows: vec![flowy_agent::ChatLine::Artifact {
                        artifact_id: "art-empty-ws".into(),
                    }],
                    streaming: None,
                    status: flowy_agent::TurnStatus::Completed,
                },
                cx,
            );
        });
        panel.toggle_deliverables(cx);
        assert!(panel.right_drawer_open);
        assert_eq!(panel.visible_workspace_deliverables(cx).len(), 0);
    });
    cx.run_until_parked();
    notifies.store(0, Ordering::SeqCst);

    let item = crate::library::LibraryItem {
        id: "art-empty-ws".into(),
        title: "Summary Deck".into(),
        original_name: "summary.pptx".into(),
        description: String::new(),
        category: crate::library::Category::Slides,
        date: String::new(),
        time: String::new(),
        pending: false,
        preview: None,
        size_bytes: Some(2048),
        extension: "pptx".into(),
        media_type: None,
        delivered_at_ms: 0,
        delivered_date: "2026-11-27".into(),
        delivered_time: "10:00".into(),
        source_session_id: String::new(),
        source_title: String::new(),
        source_deleted: false,
        source_unavailable: false,
        favorite: false,
        availability: crate::library::ArtifactAvailability::Available,
        override_used: false,
    };
    panel.update(cx, |panel, cx| {
        panel.set_artifacts(vec![item], cx);
    });
    cx.run_until_parked();
    assert!(
        notifies.load(Ordering::SeqCst) >= 1,
        "set_artifacts with open right drawer must notify StudioPanel even when workspace deliverable paths are empty"
    );
    panel.read_with(cx, |panel, cx| {
        assert_eq!(panel.feed.read(cx).session_artifacts().len(), 1);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn board_lines_cache_skips_cloning_outside_timeline_tab(
    cx: &mut gpui::TestAppContext,
) {
    let root = std::env::temp_dir().join(format!(
        "flowy-panel-board-lines-cache-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create test root");

    let (panel, cx) = cx.add_window_view(|window, cx| {
        let ui = UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        StudioPanel::new(
            StudioConfig {
                product: "Flowy",
                i18n: I18n::embedded("Flowy"),
                model: Model::Lite,
                plan: Plan::Free,
                model_info_version: String::new(),
                model_info_open: false,
                store_root: root.clone(),
                llm: LlmConfig::default(),
            },
            ui,
            window,
            cx,
        )
    });

    panel.update(cx, |panel, cx| {
        let sid = panel.agent.create_session().expect("create session");
        panel.open_board(cx);
        assert_eq!(
            panel.board_inspector.session.as_deref(),
            Some(sid.as_str())
        );
        assert!(
            panel.cached_board_lines.is_none(),
            "Overview tab must not clone cached_board_lines"
        );

        panel
            .select_board_tab(crate::overview::BoardInspectorTab::Timeline, cx);
        assert_eq!(
            panel.cached_board_lines.as_ref().map(|(id, _)| id.as_str()),
            Some(sid.as_str()),
            "Timeline tab must populate cached_board_lines"
        );

        panel.select_board_tab(crate::overview::BoardInspectorTab::Trace, cx);
        assert!(
            panel.cached_board_lines.is_none(),
            "leaving Timeline tab must drop cached_board_lines"
        );
    });

    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn board_preview_failed_gate_uses_inspected_board_session_not_chat_feed(
    cx: &mut gpui::TestAppContext,
) {
    let root = std::env::temp_dir().join(format!(
        "flowy-panel-board-preview-gate-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create test root");

    let chat_failed_file = root.join("chat_failed_board_ok.md");
    let board_failed_file = root.join("board_failed.md");
    std::fs::write(&chat_failed_file, "# ok on board").expect("write md");
    std::fs::write(&board_failed_file, "# failed on board").expect("write md");
    let chat_failed_str = chat_failed_file.to_string_lossy().into_owned();
    let board_failed_str = board_failed_file.to_string_lossy().into_owned();

    let (panel, cx) = cx.add_window_view(|window, cx| {
        let ui = UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        StudioPanel::new(
            StudioConfig {
                product: "Flowy",
                i18n: I18n::embedded("Flowy"),
                model: Model::Lite,
                plan: Plan::Free,
                model_info_version: String::new(),
                model_info_open: false,
                store_root: root.clone(),
                llm: LlmConfig::default(),
            },
            ui,
            window,
            cx,
        )
    });

    panel.update(cx, |panel, cx| {
        panel.feed.update(cx, |feed, cx| {
            feed.sync_history(
                Some("chat-sess".into()),
                flowy_agent::ChatHistory {
                    rows: vec![flowy_agent::ChatLine::Verification {
                        report_id: "rep-chat".into(),
                        artifact_path: chat_failed_str.clone(),
                        artifact_hash: "0000".into(),
                        ok: false,
                        issues: vec![flowy_agent::VerificationIssue {
                            code: "formula_error".into(),
                            field: "file".into(),
                        }],
                        semantic_hint: None,
                        override_used: false,
                        provenance_warnings: Vec::new(),
                        source_session_id: None,
                    }],
                    streaming: None,
                    status: flowy_agent::TurnStatus::Completed,
                },
                cx,
            );
        });

        panel.surface = StudioSurface::Board;
        panel.board_snapshot.sessions =
            vec![crate::ports::SessionBoardGroup {
                session_id: "s-board".into(),
                title: "Board Session".into(),
                project_id: None,
                project_name: None,
                workspace_display: String::new(),
                updated_at: 1_700_000_000_000,
                pinned: false,
                archived: false,
                preview: None,
                assistant_summary: None,
                branches: vec![
                    crate::ports::WorkBranchSummary {
                        id: "b-pass".into(),
                        owner_session_id: None,
                        skill_label: crate::ports::SkillLabel::Key(
                            "board.skill_work",
                        ),
                        phase: crate::ports::BranchPhase::Delivered {
                            override_used: false,
                            hash_short: "abcd".into(),
                            provenance_warnings: Vec::new(),
                        },
                        deliverable_name: "chat_failed_board_ok.md".into(),
                        deliverable_path: Some(chat_failed_str.clone()),
                        format: crate::ports::DeliverableFormat::Markdown,
                        is_workspace_change: false,
                        progress_pct: 100.0,
                        detail: String::new(),
                        work_progress: None,
                    },
                    crate::ports::WorkBranchSummary {
                        id: "b-fail".into(),
                        owner_session_id: None,
                        skill_label: crate::ports::SkillLabel::Key(
                            "board.skill_work",
                        ),
                        phase: crate::ports::BranchPhase::NeedsApproval {
                            report_id: "rep-board".into(),
                            artifact_path: board_failed_str.clone(),
                            summary: "failed".into(),
                            issues: Vec::new(),
                            semantic_hint: None,
                            provenance_warnings: Vec::new(),
                        },
                        deliverable_name: "board_failed.md".into(),
                        deliverable_path: Some(board_failed_str.clone()),
                        format: crate::ports::DeliverableFormat::Markdown,
                        is_workspace_change: false,
                        progress_pct: 50.0,
                        detail: String::new(),
                        work_progress: None,
                    },
                ],
                workspace_changes: Vec::new(),
                child_workers: Vec::new(),
                total_branch_count: 2,
                total_file_count: 2,
                session_contract: None,
                has_blocker: true,
                has_delivered: true,
                is_running: false,
                is_active: true,
                group_kpi: crate::ports::BoardKpiSummary::default(),
            }];
        panel.board_inspector.session = Some("s-board".into());

        panel.set_preview_file(Some(chat_failed_str.clone()), cx);
        assert_eq!(
            panel.drawer_preview.file.as_deref(),
            Some(chat_failed_str.as_str()),
            "unrelated chat failure must not block previewing passed board deliverable"
        );

        panel.set_preview_file(Some(board_failed_str), cx);
        assert!(
            panel.drawer_preview.file.is_none(),
            "NeedsApproval deliverable on inspected board session must be blocked"
        );
        assert_eq!(panel.notice, Some("verify.card.failed"));
    });

    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn quote_file_from_board_opens_inspected_session_and_attaches_quote(
    cx: &mut gpui::TestAppContext,
) {
    let root = std::env::temp_dir()
        .join(format!("flowy-panel-board-quote-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create test root");

    let (panel, cx) = cx.add_window_view(|window, cx| {
        let ui = UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
        StudioPanel::new(
            StudioConfig {
                product: "Flowy",
                i18n: I18n::embedded("Flowy"),
                model: Model::Lite,
                plan: Plan::Free,
                model_info_version: String::new(),
                model_info_open: false,
                store_root: root.clone(),
                llm: LlmConfig::default(),
            },
            ui,
            window,
            cx,
        )
    });

    panel.update(cx, |panel, cx| {
        let sid_a = panel.agent.create_session().expect("session A");
        let sid_b = panel.agent.create_session().expect("session B");
        panel.open_session(&sid_b, cx);
        assert_eq!(
            panel.agent.active_session_id().as_deref(),
            Some(sid_b.as_str())
        );

        panel.open_board(cx);
        panel.board_inspector.session = Some(sid_a.clone());
        panel.board_inspector.drawer_open = true;
        panel.drawer_preview.file = Some("/tmp/q.md".into());

        panel.quote_file_to_composer("/tmp/q.md", None, cx);
        assert_ne!(panel.surface, StudioSurface::Board);
        assert!(!panel.board_inspector.drawer_open);
        assert!(panel.drawer_preview.file.is_none());
        assert_eq!(
            panel.agent.active_session_id().as_deref(),
            Some(sid_a.as_str()),
            "quoting from Board must switch active session to inspected session A"
        );
        assert!(
            panel
                .active_quote
                .as_ref()
                .is_some_and(|q| q.excerpt.contains("q.md")),
            "attached file quote must survive transition from Board"
        );
    });

    drop(panel);
    let _ = std::fs::remove_dir_all(root);
}
