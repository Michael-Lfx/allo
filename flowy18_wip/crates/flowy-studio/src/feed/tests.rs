use super::*;
use flowy_ui as theme;
use gpui::{ListOffset, TestAppContext};

#[gpui::test]
fn durable_artifact_anchor_resolves_the_projected_row_in_long_history(
    cx: &mut TestAppContext,
) {
    let (feed, cx) = cx.add_window_view(|_, cx| {
        let ui = UiContext::new(theme::ThemeOptions::default(), false, cx);
        ChatFeed::new(ui, I18n::embedded("Flowy"), Some("source".into()), cx)
    });
    let mut rows = Vec::new();
    for index in 0..500 {
        rows.push(ChatLine::User {
            text: format!("request {index}"),
        });
        rows.push(ChatLine::Assistant {
            text: format!("answer {index}"),
        });
    }
    rows.extend([
        ChatLine::User {
            text: "make report".into(),
        },
        ChatLine::ToolResult {
            text: "write".into(),
            is_error: false,
        },
        ChatLine::ToolResult {
            text: "publish".into(),
            is_error: false,
        },
        ChatLine::Assistant {
            text: "Report ready".into(),
        },
        ChatLine::Artifact {
            artifact_id: "report-id".into(),
        },
    ]);
    feed.update(cx, |feed, cx| {
        feed.sync_history(Some("source".into()), ChatHistory { rows, status: TurnStatus::Completed, streaming: None }, cx);
        assert!(feed.focus_artifact("report-id", cx));
        let projected = feed.list.logical_scroll_top().item_ix;
        assert!(matches!(&feed.lines[projected], ChatLine::Artifact { artifact_id } if artifact_id == "report-id"));
        assert_eq!(feed.projection.source_rows[projected], 1004);
        assert!(feed.artifact_focus("report-id").is_some());
        assert!(!feed.focus_artifact("absent-id", cx));
    });
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured = events.clone();
    let _subscription = feed.update(cx, |_, cx| {
        cx.subscribe(&feed, move |_, _, event: &ArtifactAction, _| {
            captured.lock().unwrap().push(event.clone());
        })
    });
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
    }
    let focus = cx.read_entity(&feed, |feed, _| {
        feed.artifact_focus("report-id").unwrap()
    });
    cx.update(|window, cx| {
        assert_eq!(window.focused(cx), Some(focus));
    });
    // The row is still a real pointer target: clicking the painted control
    // must dispatch the same action the platform accessibility click did.
    let bounds = cx
        .debug_bounds("chat-artifact-preview")
        .expect("the artifact row paints a named preview control");
    assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
    cx.simulate_click(bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert!(
        matches!(events.lock().unwrap().as_slice(), [ArtifactAction::Preview(id)] if id == "report-id")
    );
}

#[gpui::test]
fn folding_a_read_detail_returns_to_its_turn_control(cx: &mut TestAppContext) {
    let (feed, cx) = cx.add_window_view(|_, cx| {
        let ui = UiContext::new(theme::ThemeOptions::default(), false, cx);
        ChatFeed::new(ui, I18n::embedded("Flowy"), Some("fold".into()), cx)
    });
    let mut rows = vec![
        ChatLine::User {
            text: "request".into(),
        },
        ChatLine::Tool {
            text: "first detail".into(),
        },
        ChatLine::Tool {
            text: "second detail".into(),
        },
        ChatLine::Assistant {
            text: "**answer**".into(),
        },
    ];
    rows.extend((0..100).map(|id| ChatLine::User {
        text: format!("next {id}"),
    }));
    feed.update(cx, |feed, cx| {
        feed.sync_history(
            Some("fold".into()),
            ChatHistory {
                rows,
                streaming: None,
                status: TurnStatus::Completed,
            },
            cx,
        );
        assert!(!feed.projection.folds[&1].expanded);
        feed.list.scroll_to(ListOffset {
            item_ix: 1,
            offset_in_item: px(0.),
        });
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    // The fold control is a button; clicking the painted row must expand it.
    let fold = cx
        .debug_bounds("chat-turn-fold")
        .expect("the collapsed turn paints its fold control");
    cx.simulate_click(fold.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    feed.update(cx, |feed, cx| {
        assert!(feed.projection.folds[&1].expanded);
        let detail_id = feed.row_ids[2];
        feed.toggle_detail(detail_id, cx);
        assert!(feed.expanded_rows.contains(&detail_id));
        feed.list.scroll_to(ListOffset {
            item_ix: 2,
            offset_in_item: px(7.),
        });
        feed.set_turn_expanded(1, false, cx);
        let top = feed.list.logical_scroll_top();
        assert_eq!(top.item_ix, 1);
        assert_eq!(top.offset_in_item, px(0.));
        assert!(!feed.row_focus.contains_key(&detail_id));
        assert!(!feed.expanded_rows.contains(&detail_id));
    });
}

#[gpui::test]
#[expect(
    clippy::needless_pass_by_ref_mut,
    reason = "GPUI test harness passes &mut TestAppContext"
)]
fn working_indicator_keeps_and_clears_the_error_state(cx: &mut TestAppContext) {
    let indicator = cx.update(|cx| {
        let ui = UiContext::new(theme::ThemeOptions::default(), false, cx);
        cx.new(|cx| WorkingIndicator::new(ui, cx))
    });
    cx.update(|cx| {
        indicator.update(cx, |working, cx| {
            working.set_label("boom".into(), true, true, cx);
        });
    });
    cx.read_entity(&indicator, |working, _| {
        assert!(working.visible);
        assert!(working.error);
        assert_eq!(working.label, "boom");
    });
    cx.update(|cx| {
        indicator.update(cx, |working, cx| {
            working.set_label("working".into(), true, false, cx);
            working.set_retry(true, "Retry delivery".into(), cx);
        });
    });
    cx.read_entity(&indicator, |working, _| {
        assert!(working.visible);
        assert!(!working.error);
        assert_eq!(working.label, "working");
        assert!(working.retry);
        assert_eq!(working.retry_label, "Retry delivery");
    });
}

#[gpui::test]
fn thousand_rows_are_lazy_and_session_scroll_is_restored(
    cx: &mut TestAppContext,
) {
    let (feed, cx) = cx.add_window_view(|_, cx| {
        let ui = UiContext::new(theme::ThemeOptions::default(), false, cx);
        let rows = (0..1_000)
            .map(|i| ChatLine::Assistant {
                text: format!("Message {i}"),
            })
            .collect();
        let mut feed = ChatFeed::new(
            ui,
            I18n::embedded("Flowy"),
            Some("first".into()),
            cx,
        );
        feed.sync_rows(Some("first".into()), rows, cx);
        feed
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.read_entity(&feed, |feed, _| {
        assert!(
            feed.rendered > 0 && feed.rendered < 100,
            "rendered {} rows",
            feed.rendered
        );
    });
    cx.update(|_, cx| {
        feed.update(cx, |feed, cx| {
            feed.list.scroll_to(ListOffset {
                item_ix: 450,
                offset_in_item: px(3.),
            });
            let first = feed.lines.clone();
            feed.sync_rows(
                Some("second".into()),
                vec![ChatLine::User {
                    text: "other".into(),
                }],
                cx,
            );
            feed.sync_rows(Some("first".into()), first, cx);
            assert_eq!(feed.list.logical_scroll_top().item_ix, 450);
            assert_eq!(feed.list.logical_scroll_top().offset_in_item, px(3.));
        });
    });
    let focus = cx
        .read_entity(&feed, |feed, _| feed.row_focus[&feed.row_ids[0]].clone());
    cx.update(|window, cx| {
        focus.focus(window, cx);
        feed.update(cx, |feed, _| {
            feed.rendered_rows.clear();
        });
        window.draw(cx).clear(cx);
    });
    cx.read_entity(&feed, |feed, _| {
        assert!(
            feed.rendered_rows.contains(&feed.row_ids[0]),
            "a focused row stays rendered outside the viewport"
        );
        assert!(feed.rendered_rows.len() < 100);
    });
}

#[gpui::test]
fn streaming_multiline_prepend_and_presentation_changes_keep_row_identity(
    cx: &mut TestAppContext,
) {
    let (feed, cx) = cx.add_window_view(|_, cx| {
        let ui = UiContext::new(theme::ThemeOptions::default(), false, cx);
        let mut feed = ChatFeed::new(
            ui,
            I18n::embedded("Flowy"),
            Some("first".into()),
            cx,
        );
        feed.sync_rows(
            Some("first".into()),
            vec![
                ChatLine::User {
                    text: "Question".into(),
                },
                ChatLine::Assistant {
                    text: "Answer".into(),
                },
                ChatLine::ToolCall {
                    name: "read".into(),
                    target: "file.rs".into(),
                    call_id: "call-1".into(),
                    running: true,
                },
            ]
            .into_iter()
            .chain((0..100).map(|i| ChatLine::Assistant {
                text: format!("History row {i}"),
            }))
            .collect(),
            cx,
        );
        feed
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    // Use a scrollable history and explicitly reveal the streamed row;
    // a bottom-aligned list shorter than its viewport has no logical top row.
    cx.update(|_, cx| {
        feed.update(cx, |feed, cx| {
            feed.list.scroll_to(ListOffset {
                item_ix: 1,
                offset_in_item: px(0.),
            });
            cx.notify();
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let original_ids = cx.read_entity(&feed, |feed, _| feed.row_ids.clone());
    cx.update(|_, cx| {
        feed.update(cx, |feed, cx| {
            feed.list.scroll_to(ListOffset {
                item_ix: 1,
                offset_in_item: px(3.),
            });
            let mut rows = feed.lines.clone();
            rows[1] = ChatLine::Assistant {
                text: format!(
                    "First line\nSecond line\n{}",
                    "streamed words ".repeat(220)
                ),
            };
            rows[2] = ChatLine::ToolCall {
                name: "read".into(),
                target: "file.rs".into(),
                call_id: "call-1".into(),
                running: false,
            };
            feed.sync_rows(Some("first".into()), rows, cx);
            assert_eq!(feed.row_ids, original_ids);
            assert_eq!(feed.list.logical_scroll_top().item_ix, 1);
            assert_eq!(feed.list.logical_scroll_top().offset_in_item, px(3.));
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.read_entity(&feed, |feed, _| {
        // GPUI's TestTextSystem deliberately reports one visual line for
        // all text. Native wrapping height is covered by the platform
        // visual matrix; here we verify the row remains measured and its
        // identity/scroll anchor survive streaming updates.
        assert!(feed.list.bounds_for_item(1).is_some());
    });
    cx.update(|_, cx| {
        feed.update(cx, |feed, cx| {
            feed.list.scroll_to(ListOffset {
                item_ix: 1,
                offset_in_item: px(3.),
            });
            let mut rows = feed.lines.clone();
            rows.insert(
                0,
                ChatLine::Status {
                    text: "Earlier history".into(),
                },
            );
            feed.sync_rows(Some("first".into()), rows, cx);
            assert_eq!(&feed.row_ids[1..], original_ids.as_slice());
            assert_eq!(feed.list.logical_scroll_top().item_ix, 2);
            assert_eq!(feed.list.logical_scroll_top().offset_in_item, px(3.));
            let mut i18n = I18n::embedded("Flowy");
            i18n.set_locale(flowy_i18n::Locale::Ar);
            feed.sync_i18n(i18n, cx);
            feed.theme = ResolvedTheme::resolve(
                theme::ThemeOptions {
                    mode: theme::ThemeMode::Dark,
                    ..theme::ThemeOptions::default()
                },
                false,
            );
            feed.list.remeasure_items(0..feed.lines.len());
            assert_eq!(&feed.row_ids[1..], original_ids.as_slice());
            assert_eq!(feed.list.logical_scroll_top().item_ix, 2);
            assert_eq!(feed.list.logical_scroll_top().offset_in_item, px(3.));
        });
    });
}

#[gpui::test]
fn streaming_markdown_transition_keeps_row_identity_and_scroll_anchor(
    cx: &mut TestAppContext,
) {
    let mut base_rows: Vec<ChatLine> = (0..60)
        .map(|i| ChatLine::Assistant {
            text: format!("Earlier reply {i}"),
        })
        .collect();
    base_rows.push(ChatLine::User {
        text: "Build a launch page".into(),
    });
    base_rows.push(ChatLine::Assistant {
        text: "**Working** on it\n```html\n<main>draft".into(),
    });

    let initial_rows = base_rows.clone();
    let tail_ix = initial_rows.len() - 1;
    let (feed, cx) = cx.add_window_view(|_, cx| {
        let ui = UiContext::new(theme::ThemeOptions::default(), false, cx);
        let mut feed = ChatFeed::new(
            ui,
            I18n::embedded("Flowy"),
            Some("stream-md".into()),
            cx,
        );
        feed.sync_history(
            Some("stream-md".into()),
            ChatHistory {
                rows: initial_rows,
                streaming: Some(tail_ix),
                status: TurnStatus::Running,
            },
            cx,
        );
        feed
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // User scrolls up away from the streaming tail to read earlier history.
    cx.update(|_, cx| {
        feed.update(cx, |feed, cx| {
            feed.list.scroll_to(ListOffset {
                item_ix: 12,
                offset_in_item: px(4.),
            });
            cx.notify();
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let streaming_ids = cx.read_entity(&feed, |feed, _| feed.row_ids.clone());
    assert!(
        cx.read_entity(&feed, |feed, _| feed.projection.streaming.is_some()),
        "tail row should be projected as streaming"
    );

    // Stream finishes and closes the code fence: row identity and scrolled-away
    // anchor must remain untouched when transitioning from render_streaming
    // to full markdown::render.
    let mut finished_rows = base_rows;
    *finished_rows.last_mut().unwrap() = ChatLine::Assistant {
        text: "**Done** building\n```html\n<main>final</main>\n```".into(),
    };
    cx.update(|_, cx| {
        feed.update(cx, |feed, cx| {
            feed.sync_history(
                Some("stream-md".into()),
                ChatHistory {
                    rows: finished_rows,
                    streaming: None,
                    status: TurnStatus::Idle,
                },
                cx,
            );
            assert_eq!(feed.row_ids, streaming_ids);
            assert_eq!(feed.list.logical_scroll_top().item_ix, 12);
            assert_eq!(feed.list.logical_scroll_top().offset_in_item, px(4.));
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.read_entity(&feed, |feed, _| {
        assert!(feed.projection.streaming.is_none());
        assert_eq!(feed.list.logical_scroll_top().item_ix, 12);
        assert_eq!(feed.list.logical_scroll_top().offset_in_item, px(4.));
    });
}

#[gpui::test]
fn verification_card_renders_four_codes_hides_default_save_and_requires_checkbox(
    cx: &mut TestAppContext,
) {
    use std::sync::{Arc, Mutex};

    let i18n = I18n::embedded("Flowy");
    let events = Arc::new(Mutex::new(Vec::new()));
    let issues = vec![
        flowy_agent::VerificationIssue {
            code: "ANCHOR_MISSING".into(),
            field: "S0-title".into(),
        },
        flowy_agent::VerificationIssue {
            code: "ANCHOR_DUPLICATE".into(),
            field: "S0-body".into(),
        },
        flowy_agent::VerificationIssue {
            code: "ANCHOR_INVALID".into(),
            field: "S0-extra".into(),
        },
        flowy_agent::VerificationIssue {
            code: "VALUE_MISMATCH".into(),
            field: "S1-body".into(),
        },
    ];
    let sample_issues = issues.clone();
    let (feed, cx) = cx.add_window_view(|_, cx| {
        let ui = UiContext::new(theme::ThemeOptions::default(), false, cx);
        let mut feed =
            ChatFeed::new(ui, i18n.clone(), Some("verify".into()), cx);
        feed.sync_rows(
            Some("verify".into()),
            vec![ChatLine::Verification {
                report_id: "report-42".into(),
                artifact_path: "outputs/sub/deck.pptx".into(),
                artifact_hash: "a1b2c3d4e5f60000".into(),
                ok: false,
                issues: sample_issues,
                semantic_hint: Some("标题与正文一致".into()),
                override_used: false,
                provenance_warnings: Vec::new(),
                source_session_id: Some("w-child-42".into()),
            }],
            cx,
        );
        feed
    });
    let sink = Arc::clone(&events);
    let _sub = cx.update(|_, cx| {
        cx.subscribe(&feed, move |_, action: &ArtifactAction, _| {
            sink.lock().unwrap().push(action.clone());
        })
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    // Verify distinct localized copy across both zh-CN and en.
    cx.read_entity(&feed, |feed, _| {
        let zh = feed.format_verification_issues(&issues);
        let unique_zh: std::collections::HashSet<&String> = zh.iter().collect();
        assert_eq!(unique_zh.len(), 4);
        assert!(zh[0].contains(&i18n.t("verify.anchor_missing")));
        assert!(zh[3].contains(&i18n.t("verify.value_mismatch")));
    });
    cx.update(|_, cx| {
        feed.update(cx, |feed, cx| {
            let mut en = I18n::embedded("Flowy");
            en.set_locale(flowy_i18n::Locale::En);
            feed.sync_i18n(en, cx);
            let formatted = feed.format_verification_issues(&issues);
            let unique_en: std::collections::HashSet<&String> =
                formatted.iter().collect();
            assert_eq!(unique_en.len(), 4);
        });
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    assert!(
        cx.debug_bounds("chat-verification-card").is_some(),
        "verification card must render"
    );
    assert!(
        cx.debug_bounds("chat-artifact-save-as").is_none(),
        "failed verification card must never render ordinary save/deliver button"
    );

    // Clicking submit before checking the override box does nothing.
    let submit = cx
        .debug_bounds("chat-verify-override-submit")
        .expect("override submit control rendered");
    cx.simulate_click(submit.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert_eq!(events.lock().unwrap().len(), 0);

    // Checking the override box unlocks the explicit override action.
    let check = cx
        .debug_bounds("chat-verify-override-check")
        .expect("override checkbox rendered");
    cx.simulate_click(check.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let submit = cx
        .debug_bounds("chat-verify-override-submit")
        .expect("override submit control rendered");
    cx.simulate_click(submit.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert!(matches!(
        events.lock().unwrap().as_slice(),
        [ArtifactAction::OverrideDeliver {
            session_id,
            report_id,
            artifact_path,
        }] if session_id.as_deref() == Some("w-child-42")
            && report_id == "report-42"
            && artifact_path == "outputs/sub/deck.pptx"
    ));
}

#[test]
fn latest_deliverables_includes_verified_and_overridden_but_excludes_failed() {
    let failed_only = vec![
        ChatLine::User {
            text: "build deck".into(),
        },
        ChatLine::Verification {
            report_id: "r1".into(),
            artifact_path: "outputs/bad.pptx".into(),
            artifact_hash: "aaaa".into(),
            ok: false,
            issues: Vec::new(),
            semantic_hint: None,
            override_used: false,
            provenance_warnings: Vec::new(),
            source_session_id: None,
        },
    ];
    assert!(collect_latest_deliverables(&failed_only).is_none());

    let verified_and_overridden = vec![
        ChatLine::User {
            text: "build decks".into(),
        },
        ChatLine::Verification {
            report_id: "r2".into(),
            artifact_path: "outputs/ok.pptx".into(),
            artifact_hash: "bbbb".into(),
            ok: true,
            issues: Vec::new(),
            semantic_hint: None,
            override_used: false,
            provenance_warnings: Vec::new(),
            source_session_id: None,
        },
        ChatLine::Verification {
            report_id: "r3".into(),
            artifact_path: "outputs/override.docx".into(),
            artifact_hash: "cccc".into(),
            ok: false,
            issues: Vec::new(),
            semantic_hint: None,
            override_used: true,
            provenance_warnings: Vec::new(),
            source_session_id: None,
        },
        ChatLine::Artifact {
            artifact_id: "a1".into(),
        },
    ];
    let found = collect_latest_deliverables(&verified_and_overridden)
        .expect("verified and overridden deliverables must surface");
    let paths: Vec<&str> = found.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, vec!["outputs/ok.pptx", "outputs/override.docx"]);
}

#[gpui::test]
fn collapsed_passed_verification_card_still_shows_l3_semantic_hint(
    cx: &mut TestAppContext,
) {
    let i18n = I18n::embedded("Flowy");
    let (_feed, cx) = cx.add_window_view(|_, cx| {
        let ui = UiContext::new(theme::ThemeOptions::default(), false, cx);
        let mut feed =
            ChatFeed::new(ui, i18n.clone(), Some("verify-l3".into()), cx);
        feed.sync_rows(
            Some("verify-l3".into()),
            vec![ChatLine::Verification {
                report_id: "report-l3".into(),
                artifact_path: "outputs/sub/east.pptx".into(),
                artifact_hash: "1122334455667788".into(),
                ok: true,
                issues: Vec::new(),
                semantic_hint: Some("第1页写成华南区".into()),
                override_used: false,
                provenance_warnings: Vec::new(),
                source_session_id: None,
            }],
            cx,
        );
        feed
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    assert!(
        cx.debug_bounds("chat-verify-semantic-hint").is_some(),
        "L3 semantic hint must remain visible on collapsed passed cards"
    );
}

#[gpui::test]
fn trailing_stopped_row_suppressed_only_when_handoff_manifest_present(
    cx: &mut TestAppContext,
) {
    let i18n = I18n::embedded("Flowy");
    let stopped_history = || ChatHistory {
        rows: vec![
            ChatLine::User {
                text: "stop me".into(),
            },
            ChatLine::Status {
                text: "chat.status_stopped".into(),
            },
        ],
        status: TurnStatus::Stopped,
        streaming: None,
    };
    let (feed, cx) = cx.add_window_view(|_, cx| {
        let ui = UiContext::new(theme::ThemeOptions::default(), false, cx);
        ChatFeed::new(ui, i18n.clone(), Some("s-stop".into()), cx)
    });

    // Without a stop-handoff manifest, the stopped status row stays visible
    // so fast cancels and restored sessions never look silent.
    feed.update(cx, |feed, cx| {
        feed.set_suppress_trailing_stopped(false);
        feed.sync_history(Some("s-stop".into()), stopped_history(), cx);
        assert_eq!(feed.lines.len(), 2);
    });

    // When a detailed stop-handoff card is visible right below the feed,
    // projection omits the duplicate trailing row completely (zero empty row).
    feed.update(cx, |feed, cx| {
        feed.set_suppress_trailing_stopped(true);
        feed.sync_history(Some("s-stop".into()), stopped_history(), cx);
        assert_eq!(feed.lines.len(), 1);
        assert!(matches!(feed.lines[0], ChatLine::User { .. }));
    });

    // Once a follow-up turn starts, historical stopped status stays in transcript.
    feed.update(cx, |feed, cx| {
        let mut next = stopped_history();
        next.rows.push(ChatLine::User {
            text: "continue".into(),
        });
        next.status = TurnStatus::Running;
        feed.set_suppress_trailing_stopped(true);
        feed.sync_history(Some("s-stop".into()), next, cx);
        assert_eq!(feed.lines.len(), 3);
        assert!(matches!(
            &feed.lines[1],
            ChatLine::Status { text } if text == "chat.status_stopped"
        ));
    });
}

#[test]
fn collect_latest_deliverables_hides_pass_overturned_by_later_failure() {
    let lines = vec![
        ChatLine::User {
            text: "Build report".into(),
        },
        ChatLine::FileChange {
            path: r"C:\项目\报告.md".into(),
            change: flowy_agent::FileChangeKind::Add,
            added: 10,
            removed: 0,
            binary: false,
        },
        ChatLine::Verification {
            report_id: "rep-1".into(),
            artifact_path: r"C:\项目\报告.md".into(),
            artifact_hash: "h1".into(),
            ok: true,
            issues: Vec::new(),
            semantic_hint: None,
            override_used: false,
            provenance_warnings: Vec::new(),
            source_session_id: None,
        },
        ChatLine::Verification {
            report_id: "rep-2".into(),
            artifact_path: r"C:\项目\报告.md".into(),
            artifact_hash: "h2".into(),
            ok: false,
            issues: vec![flowy_agent::VerificationIssue {
                code: "BROKEN".into(),
                field: "body".into(),
            }],
            semantic_hint: None,
            override_used: false,
            provenance_warnings: Vec::new(),
            source_session_id: None,
        },
    ];
    assert!(
        collect_latest_deliverables(&lines).is_none(),
        "earlier pass overturned by later failure must never enter deliverables drawer"
    );
}

#[test]
fn collect_latest_deliverables_surfaces_fail_then_pass_and_override_once() {
    let fail_then_pass = vec![
        ChatLine::User {
            text: "Build report".into(),
        },
        ChatLine::Verification {
            report_id: "rep-1".into(),
            artifact_path: r"C:\Work\summary.docx".into(),
            artifact_hash: "h1".into(),
            ok: false,
            issues: vec![flowy_agent::VerificationIssue {
                code: "EMPTY".into(),
                field: "doc".into(),
            }],
            semantic_hint: None,
            override_used: false,
            provenance_warnings: Vec::new(),
            source_session_id: None,
        },
        ChatLine::Verification {
            report_id: "rep-2".into(),
            artifact_path: r"C:\Work\summary.docx".into(),
            artifact_hash: "h2".into(),
            ok: true,
            issues: Vec::new(),
            semantic_hint: None,
            override_used: false,
            provenance_warnings: Vec::new(),
            source_session_id: None,
        },
    ];
    let entries = collect_latest_deliverables(&fail_then_pass)
        .expect("fail then pass settles to deliverable");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, r"C:\Work\summary.docx");

    let pass_fail_override = vec![
        ChatLine::User {
            text: "Build report".into(),
        },
        ChatLine::Verification {
            report_id: "rep-1".into(),
            artifact_path: r"C:\Work\summary.docx".into(),
            artifact_hash: "h1".into(),
            ok: true,
            issues: Vec::new(),
            semantic_hint: None,
            override_used: false,
            provenance_warnings: Vec::new(),
            source_session_id: None,
        },
        ChatLine::Verification {
            report_id: "rep-2".into(),
            artifact_path: r"C:\Work\summary.docx".into(),
            artifact_hash: "h2".into(),
            ok: false,
            issues: Vec::new(),
            semantic_hint: None,
            override_used: false,
            provenance_warnings: Vec::new(),
            source_session_id: None,
        },
        ChatLine::Verification {
            report_id: "rep-3".into(),
            artifact_path: r"\\?\C:\Work\summary.docx".into(),
            artifact_hash: "h2".into(),
            ok: false,
            issues: Vec::new(),
            semantic_hint: None,
            override_used: true,
            provenance_warnings: Vec::new(),
            source_session_id: None,
        },
    ];
    let overridden = collect_latest_deliverables(&pass_fail_override)
        .expect("explicit override settles deliverable once");
    assert_eq!(overridden.len(), 1);
}

#[test]
fn collect_latest_deliverables_isolates_latest_turn_from_earlier_anchored_cards()
 {
    let lines = vec![
        ChatLine::User {
            text: "deliver alpha".into(),
        },
        ChatLine::Verification {
            report_id: "rep-a".into(),
            artifact_path: r"C:\Work\alpha.pptx".into(),
            artifact_hash: "ha".into(),
            ok: true,
            issues: Vec::new(),
            semantic_hint: None,
            override_used: false,
            provenance_warnings: Vec::new(),
            source_session_id: Some("w-1".into()),
        },
        ChatLine::User {
            text: "deliver beta".into(),
        },
        ChatLine::Verification {
            report_id: "rep-b".into(),
            artifact_path: r"C:\Work\beta.pptx".into(),
            artifact_hash: "hb".into(),
            ok: true,
            issues: Vec::new(),
            semantic_hint: None,
            override_used: false,
            provenance_warnings: Vec::new(),
            source_session_id: Some("w-2".into()),
        },
    ];
    let entries = collect_latest_deliverables(&lines)
        .expect("latest turn with deliverables surfaces only beta");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, r"C:\Work\beta.pptx");
}

#[gpui::test]
fn dot_bubbles_omit_legacy_header_and_restrict_actions_to_final_reply(
    cx: &mut TestAppContext,
) {
    let i18n = I18n::embedded("Flowy");
    let mid_turn_rows = vec![
        ChatLine::User {
            text: "帮我汇总深圳天气".into(),
        },
        ChatLine::Assistant {
            text: "我先核对天气源再整理结果。".into(),
        },
        ChatLine::Reasoning {
            text: "Inspecting local weather adapter".into(),
            duration_secs: 1,
            live: false,
        },
        ChatLine::Assistant {
            text: "中间补充：继续检索详情。".into(),
        },
        ChatLine::ToolCall {
            name: "read".into(),
            target: "weather.json".into(),
            call_id: "c1".into(),
            running: false,
        },
        ChatLine::ToolResult {
            text: "ok".into(),
            is_error: false,
        },
    ];
    let mut completed_rows = mid_turn_rows.clone();
    completed_rows.push(ChatLine::Assistant {
        text: "## 深圳天气结论\n今天晴朗，气温 24°C。".into(),
    });

    for dark in [false, true] {
        let options = theme::ThemeOptions {
            mode: if dark {
                theme::ThemeMode::Dark
            } else {
                theme::ThemeMode::Light
            },
            ..Default::default()
        };
        let (feed, cx) = cx.add_window_view(|_, cx| {
            let ui = UiContext::new(options, dark, cx);
            let mut feed =
                ChatFeed::new(ui, i18n.clone(), Some("dot-bubbles".into()), cx);
            feed.sync_history(
                Some("dot-bubbles".into()),
                ChatHistory {
                    rows: mid_turn_rows.clone(),
                    streaming: None,
                    status: TurnStatus::Running,
                },
                cx,
            );
            feed
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        // Mid-turn ack + collapsed process fold never mount a copy button.
        assert!(cx.debug_bounds("chat-user-bubble").is_some());
        assert!(cx.debug_bounds("chat-ack-bubble").is_some());
        assert!(cx.debug_bounds("chat-final-bubble").is_none());
        assert!(cx.debug_bounds("chat-copy-reply").is_none());

        // Expand process_rows mid-turn: still no card chrome or action bar on ack/process.
        feed.update(cx, |feed, cx| {
            let fold_key = feed
                .projection
                .folds
                .values()
                .next()
                .expect("process fold present")
                .key;
            feed.set_turn_expanded(fold_key, true, cx);
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(cx.debug_bounds("chat-ack-bubble").is_some());
        assert!(cx.debug_bounds("chat-final-bubble").is_none());
        assert!(cx.debug_bounds("chat-copy-reply").is_none());

        // Once final Assistant settles, both Dot bubbles and final action slot render cleanly.
        feed.update(cx, |feed, cx| {
            feed.sync_history(
                Some("dot-bubbles".into()),
                ChatHistory {
                    rows: completed_rows.clone(),
                    streaming: None,
                    status: TurnStatus::Completed,
                },
                cx,
            );
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let user_b = cx
            .debug_bounds("chat-user-bubble")
            .expect("user pill rendered");
        let ack_b = cx
            .debug_bounds("chat-ack-bubble")
            .expect("ack bubble rendered");
        let final_b = cx
            .debug_bounds("chat-final-bubble")
            .expect("final assistant bubble rendered");
        let copy_b = cx
            .debug_bounds("chat-copy-reply")
            .expect("final reply action button rendered");
        assert!(
            user_b.origin.x > ack_b.origin.x,
            "user pill aligns right of assistant bubbles (dark={dark})"
        );
        assert!(
            ack_b.bottom() < final_b.origin.y,
            "ack bubble precedes final bubble (dark={dark})"
        );
        assert!(
            copy_b.origin.y >= final_b.bottom(),
            "action button renders beneath final bubble (dark={dark})"
        );
        assert!(
            cx.debug_bounds("chat-quote-reply").is_some(),
            "final reply exposes quote action (dark={dark})"
        );
        assert!(
            cx.debug_bounds("chat-user-action-bar").is_some(),
            "user bubble mounts hover action bar (dark={dark})"
        );
        assert!(
            cx.debug_bounds("chat-user-quote").is_some(),
            "user bubble exposes quote action (dark={dark})"
        );
        assert!(
            cx.debug_bounds("chat-user-copy").is_some(),
            "user bubble exposes copy action (dark={dark})"
        );
    }
}

#[gpui::test]
fn hover_bar_keeps_fixed_height_and_emits_quote_for_user_and_final_assistant(
    cx: &mut TestAppContext,
) {
    let i18n = I18n::embedded("Flowy");
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let (feed, cx) = cx.add_window_view(|_, cx| {
        let ui = UiContext::new(theme::ThemeOptions::default(), false, cx);
        let mut feed =
            ChatFeed::new(ui, i18n.clone(), Some("quote".into()), cx);
        feed.sync_history(
            Some("quote".into()),
            ChatHistory {
                rows: vec![
                    ChatLine::User {
                        text:
                            "> \u{275d} Flowy: 上一条结论\n\n请基于上文继续核对"
                                .into(),
                    },
                    ChatLine::Assistant {
                        text: "先检查环境配置。".into(),
                    },
                    ChatLine::ToolCall {
                        name: "read".into(),
                        target: "config.toml".into(),
                        call_id: "q1".into(),
                        running: false,
                    },
                    ChatLine::ToolResult {
                        text: "ok".into(),
                        is_error: false,
                    },
                    ChatLine::Assistant {
                        text: "已完成最新汇总结论。".into(),
                    },
                ],
                streaming: None,
                status: TurnStatus::Completed,
            },
            cx,
        );
        feed
    });
    let sink = events.clone();
    let _sub = cx.update(|_, cx| {
        cx.subscribe(&feed, move |_, event: &QuoteMessage, _| {
            sink.lock().unwrap().push(event.clone());
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // User bubble parses the embedded prefix into its preview strip without jittering row height.
    assert!(cx.debug_bounds("chat-user-quote-preview").is_some());
    let user_bar_before = cx
        .debug_bounds("chat-user-action-bar")
        .expect("user bar slot rendered");
    let final_bar_before = cx
        .debug_bounds("chat-final-action-bar")
        .expect("final bar slot rendered");
    let final_bubble = cx
        .debug_bounds("chat-final-bubble")
        .expect("final bubble rendered");

    // Moving pointer onto the final bubble preserves exact fixed bar height and emits Quote on click.
    cx.simulate_mouse_move(
        final_bubble.center(),
        None,
        gpui::Modifiers::none(),
    );
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let final_bar_hovered = cx
        .debug_bounds("chat-final-action-bar")
        .expect("final bar still rendered on hover");
    assert_eq!(
        final_bar_before.size.height, final_bar_hovered.size.height,
        "hover must not change virtual row action slot height"
    );
    assert_eq!(
        user_bar_before.size.height, final_bar_before.size.height,
        "user and assistant action slots share compact height"
    );

    let assistant_quote_btn = cx
        .debug_bounds("chat-quote-reply")
        .expect("assistant quote button rendered");
    cx.simulate_click(assistant_quote_btn.center(), gpui::Modifiers::none());
    cx.run_until_parked();

    // Quoting the User bubble strips its stored `> ❝` header so nested quotes never stack.
    let user_quote_btn = cx
        .debug_bounds("chat-user-quote")
        .expect("user quote button rendered");
    cx.simulate_click(user_quote_btn.center(), gpui::Modifiers::none());
    cx.run_until_parked();

    assert_eq!(
        events.lock().unwrap().as_slice(),
        &[
            QuoteMessage {
                role: crate::composer::QuoteRole::Assistant,
                text: "已完成最新汇总结论。".into(),
            },
            QuoteMessage {
                role: crate::composer::QuoteRole::User,
                text: "请基于上文继续核对".into(),
            },
        ]
    );
}

#[gpui::test]
fn dot_bubbles_bound_wide_content_inside_narrow_viewport(
    cx: &mut TestAppContext,
) {
    let i18n = I18n::embedded("Flowy");
    let (feed, cx) = cx.add_window_view(|_, cx| {
        let ui = UiContext::new(theme::ThemeOptions::default(), false, cx);
        let mut feed =
            ChatFeed::new(ui, i18n.clone(), Some("narrow".into()), cx);
        feed.sync_history(
            Some("narrow".into()),
            ChatHistory {
                rows: vec![
                    ChatLine::User {
                        text: format!(
                            "> \u{275d} Flowy: {}\n\n请总结一下这个长表格与多列宽数据的输出效果",
                            "超级长的中文单行引用摘要预览文本不应该撑破窄窗口圆角用户气泡宽度"
                                .repeat(2)
                        ),
                    },
                    ChatLine::Assistant {
                        text: "好的".into(),
                    },
                    ChatLine::ToolCall {
                        name: "read".into(),
                        target: "report.md".into(),
                        call_id: "n1".into(),
                        running: false,
                    },
                    ChatLine::ToolResult {
                        text: "ok".into(),
                        is_error: false,
                    },
                    ChatLine::Assistant {
                        text: "| ColumnOneVeryWideHeader | ColumnTwoVeryWideHeader | ColumnThreeVeryWideHeader | ColumnFourVeryWideHeader |\n| --- | --- | --- | --- |\n| value_one_unbroken_token_1234567890 | value_two_unbroken_token_1234567890 | value_three_unbroken_token_1234567890 | value_four_unbroken_token_1234567890 |"
                            .into(),
                    },
                ],
                streaming: None,
                status: TurnStatus::Completed,
            },
            cx,
        );
        feed
    });
    cx.simulate_resize(gpui::size(px(360.), px(640.)));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let user_b = cx
        .debug_bounds("chat-user-bubble")
        .expect("user bubble rendered in narrow viewport");
    let ack_b = cx
        .debug_bounds("chat-ack-bubble")
        .expect("short ack bubble rendered in narrow viewport");
    let final_b = cx
        .debug_bounds("chat-final-bubble")
        .expect("wide final bubble rendered in narrow viewport");

    assert!(
        user_b.size.width <= px(360.),
        "user bubble with long quote must stay within 360px viewport: {:?}",
        user_b.size.width
    );
    assert!(
        ack_b.size.width < px(200.),
        "short ack bubble must stay compact instead of stretching full width: {:?}",
        ack_b.size.width
    );
    assert!(
        final_b.size.width <= px(360.),
        "wide markdown final bubble must stay bounded within 360px viewport: {:?}",
        final_b.size.width
    );
    let _ = feed;
}

#[test]
fn contextual_action_label_distinguishes_bubbles_with_utf8_snippets() {
    let first = ChatFeed::contextual_action_label(
        "引用回复".into(),
        "## 深圳今日天气摘要：多云间晴，气温 20°C – 26°C，微风舒适",
    );
    let second = ChatFeed::contextual_action_label(
        "引用回复".into(),
        "明天深圳仍以多云为主，早晚微凉建议备外套",
    );
    assert_ne!(first, second);
    assert!(first.starts_with("引用回复 \u{00b7} ## 深圳今日天气摘要"));
    assert!(first.ends_with('\u{2026}'));
    assert_eq!(
        ChatFeed::contextual_action_label("引用回复".into(), "   "),
        "引用回复"
    );
}

#[test]
fn collect_latest_deliverables_keeps_valid_deliverable_across_followup() {
    let lines = vec![
        ChatLine::User {
            text: "Build deck".into(),
        },
        ChatLine::FileChanges {
            files: vec![flowy_agent::FileEntry {
                path: r"C:\Work\deck.pptx".into(),
                change: flowy_agent::FileChangeKind::Add,
                added: 0,
                removed: 0,
                binary: true,
            }],
        },
        ChatLine::Verification {
            report_id: "rep-1".into(),
            artifact_path: r"C:\Work\deck.pptx".into(),
            artifact_hash: "h1".into(),
            ok: true,
            issues: Vec::new(),
            semantic_hint: None,
            override_used: false,
            provenance_warnings: Vec::new(),
            source_session_id: None,
        },
        ChatLine::User {
            text: "thanks, what next?".into(),
        },
        ChatLine::Assistant {
            text: "You can share the deck.".into(),
        },
    ];
    let entries = collect_latest_deliverables(&lines)
        .expect("plain follow-up keeps Turn 1 deliverable");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, r"C:\Work\deck.pptx");
}

#[test]
fn collect_latest_deliverables_hides_across_turns_when_later_turn_fails_variant_path()
 {
    let lines = vec![
        ChatLine::User {
            text: "Turn 1".into(),
        },
        ChatLine::FileChanges {
            files: vec![flowy_agent::FileEntry {
                path: r"C:\Work\Alpha.md".into(),
                change: flowy_agent::FileChangeKind::Add,
                added: 12,
                removed: 0,
                binary: false,
            }],
        },
        ChatLine::Verification {
            report_id: "rep-1".into(),
            artifact_path: r"C:\Work\Alpha.md".into(),
            artifact_hash: "h1".into(),
            ok: true,
            issues: Vec::new(),
            semantic_hint: None,
            override_used: false,
            provenance_warnings: Vec::new(),
            source_session_id: None,
        },
        ChatLine::User {
            text: "Turn 2 revise".into(),
        },
        ChatLine::Verification {
            report_id: "rep-2".into(),
            artifact_path: r"\\?\c:/work/./alpha.md".into(),
            artifact_hash: "h2".into(),
            ok: false,
            issues: vec![flowy_agent::VerificationIssue {
                code: "BROKEN".into(),
                field: "body".into(),
            }],
            semantic_hint: None,
            override_used: false,
            provenance_warnings: Vec::new(),
            source_session_id: None,
        },
    ];
    assert!(
        collect_latest_deliverables(&lines).is_none(),
        "Turn 2 failure on variant path must hide Turn 1 deliverable"
    );
}

#[test]
fn collect_latest_deliverables_keeps_independent_b_and_recovers_a_after_reverify()
 {
    let mut lines = vec![
        ChatLine::User {
            text: "Turn 1".into(),
        },
        ChatLine::FileChanges {
            files: vec![
                flowy_agent::FileEntry {
                    path: r"C:\Work\a.md".into(),
                    change: flowy_agent::FileChangeKind::Add,
                    added: 5,
                    removed: 0,
                    binary: false,
                },
                flowy_agent::FileEntry {
                    path: r"C:\Work\b.md".into(),
                    change: flowy_agent::FileChangeKind::Add,
                    added: 8,
                    removed: 0,
                    binary: false,
                },
            ],
        },
        ChatLine::Verification {
            report_id: "rep-1a".into(),
            artifact_path: r"C:\Work\a.md".into(),
            artifact_hash: "ha1".into(),
            ok: true,
            issues: Vec::new(),
            semantic_hint: None,
            override_used: false,
            provenance_warnings: Vec::new(),
            source_session_id: None,
        },
        ChatLine::Verification {
            report_id: "rep-1b".into(),
            artifact_path: r"C:\Work\b.md".into(),
            artifact_hash: "hb1".into(),
            ok: true,
            issues: Vec::new(),
            semantic_hint: None,
            override_used: false,
            provenance_warnings: Vec::new(),
            source_session_id: None,
        },
        ChatLine::User {
            text: "Turn 2".into(),
        },
        ChatLine::Verification {
            report_id: "rep-2a".into(),
            artifact_path: "c:/work/A.MD".into(),
            artifact_hash: "ha2".into(),
            ok: false,
            issues: vec![flowy_agent::VerificationIssue {
                code: "BROKEN".into(),
                field: "body".into(),
            }],
            semantic_hint: None,
            override_used: false,
            provenance_warnings: Vec::new(),
            source_session_id: None,
        },
    ];

    let only_b = collect_latest_deliverables(&lines)
        .expect("independent B remains deliverable when only A fails");
    assert_eq!(only_b.len(), 1);
    assert_eq!(only_b[0].path, r"C:\Work\b.md");

    lines.push(ChatLine::User {
        text: "Turn 3".into(),
    });
    lines.push(ChatLine::Verification {
        report_id: "rep-3a".into(),
        artifact_path: r"C:\Work\a.md".into(),
        artifact_hash: "ha3".into(),
        ok: false,
        issues: Vec::new(),
        semantic_hint: None,
        override_used: true,
        provenance_warnings: Vec::new(),
        source_session_id: None,
    });
    let recovered = collect_latest_deliverables(&lines)
        .expect("override in Turn 3 restores A");
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].path, r"C:\Work\a.md");
}

#[gpui::test]
fn feed_caches_settlement_across_session_switch_and_gates_verification_quick_open(
    cx: &mut TestAppContext,
) {
    let i18n = I18n::embedded("Flowy");
    let s1_history = ChatHistory {
        rows: vec![
            ChatLine::User {
                text: "Turn 1".into(),
            },
            ChatLine::Verification {
                report_id: "r-ok".into(),
                artifact_path: r"C:\Work\report.md".into(),
                artifact_hash: "h1".into(),
                ok: true,
                issues: Vec::new(),
                semantic_hint: None,
                override_used: false,
                provenance_warnings: Vec::new(),
                source_session_id: None,
            },
            ChatLine::User {
                text: "Turn 2".into(),
            },
            ChatLine::Verification {
                report_id: "r-fail".into(),
                artifact_path: "c:/work/REPORT.md".into(),
                artifact_hash: "h2".into(),
                ok: false,
                issues: vec![flowy_agent::VerificationIssue {
                    code: "BROKEN".into(),
                    field: "body".into(),
                }],
                semantic_hint: None,
                override_used: false,
                provenance_warnings: Vec::new(),
                source_session_id: None,
            },
        ],
        streaming: None,
        status: TurnStatus::Completed,
    };
    let s2_history = ChatHistory {
        rows: vec![
            ChatLine::User {
                text: "Clean session".into(),
            },
            ChatLine::Verification {
                report_id: "r-s2".into(),
                artifact_path: r"C:\Work\clean.md".into(),
                artifact_hash: "hc".into(),
                ok: true,
                issues: Vec::new(),
                semantic_hint: None,
                override_used: false,
                provenance_warnings: Vec::new(),
                source_session_id: None,
            },
        ],
        streaming: None,
        status: TurnStatus::Completed,
    };

    let (feed, cx) = cx.add_window_view(|_, cx| {
        let ui = UiContext::new(theme::ThemeOptions::default(), false, cx);
        let mut feed = ChatFeed::new(ui, i18n.clone(), Some("s1".into()), cx);
        feed.sync_history(Some("s1".into()), s1_history.clone(), cx);
        feed
    });

    cx.update(|window, cx| window.draw(cx).clear(cx));
    feed.read_with(cx, |feed, _| {
        assert!(feed.is_path_failed(r"C:\Work\report.md"));
        assert_eq!(feed.latest_deliverable_count(), 0);
    });
    assert!(
        cx.debug_bounds("verify-open-deliverables").is_none(),
        "overturned Turn 1 passed card must not offer quick-open deliverables button"
    );

    // Switch to s2 (quick-open button must appear), then back to cached s1
    // (triggers unchanged `stored.lines == lines` and must stay gated).
    feed.update(cx, |feed, cx| {
        feed.sync_history(Some("s2".into()), s2_history.clone(), cx);
        assert!(!feed.is_path_failed(r"C:\Work\report.md"));
        assert_eq!(feed.latest_deliverable_count(), 1);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("verify-open-deliverables").is_some(),
        "settled passed card in s2 must render quick-open deliverables button"
    );

    feed.update(cx, |feed, cx| {
        feed.sync_history(Some("s1".into()), s1_history.clone(), cx);
        assert!(
            feed.is_path_failed(r"C:\Work\report.md"),
            "switching back to cached s1 must refresh verification_settlement"
        );
        assert_eq!(feed.latest_deliverable_count(), 0);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("verify-open-deliverables").is_none(),
        "switching back to s1 must keep quick-open button hidden"
    );
}

#[test]
fn line_affects_deliverables_cache_matches_only_deliverable_and_error_rows() {
    assert!(!line_affects_deliverables_cache(&ChatLine::Assistant {
        text: "hi".into(),
    }));
    assert!(!line_affects_deliverables_cache(&ChatLine::Reasoning {
        text: "thinking".into(),
        duration_secs: 1,
        live: true,
    }));
    assert!(!line_affects_deliverables_cache(&ChatLine::ToolCall {
        name: "read_file".into(),
        target: "/tmp/a.md".into(),
        call_id: "c1".into(),
        running: true,
    }));
    assert!(!line_affects_deliverables_cache(&ChatLine::ToolResult {
        text: "ok".into(),
        is_error: false,
    }));
    assert!(!line_affects_deliverables_cache(&ChatLine::Status {
        text: "done".into(),
    }));

    assert!(line_affects_deliverables_cache(&ChatLine::User {
        text: "next".into(),
    }));
    assert!(line_affects_deliverables_cache(&ChatLine::ToolResult {
        text: "failed".into(),
        is_error: true,
    }));
    assert!(line_affects_deliverables_cache(&ChatLine::Artifact {
        artifact_id: "art-1".into(),
    }));
    assert!(line_affects_deliverables_cache(&ChatLine::Verification {
        report_id: "r1".into(),
        artifact_path: "/tmp/a.pptx".into(),
        artifact_hash: "h".into(),
        ok: true,
        issues: Vec::new(),
        semantic_hint: None,
        override_used: false,
        provenance_warnings: Vec::new(),
        source_session_id: None,
    }));
    assert!(line_affects_deliverables_cache(&ChatLine::FileChanges {
        files: Vec::new(),
    }));
}

#[gpui::test]
fn streaming_text_deltas_preserve_deliverable_cache_and_refresh_on_verdict(
    cx: &mut TestAppContext,
) {
    let i18n = I18n::embedded("Flowy");
    let (feed, cx) = cx.add_window_view(|_, cx| {
        let ui = UiContext::new(theme::ThemeOptions::default(), false, cx);
        ChatFeed::new(ui, i18n.clone(), Some("s-stream".into()), cx)
    });

    let mut rows = vec![
        ChatLine::User {
            text: "build deck".into(),
        },
        ChatLine::FileChanges {
            files: vec![flowy_agent::FileEntry {
                path: r"C:\Work\deck.pptx".into(),
                change: flowy_agent::FileChangeKind::Add,
                added: 10,
                removed: 0,
                binary: false,
            }],
        },
        ChatLine::User {
            text: "stream commentary".into(),
        },
        ChatLine::Assistant {
            text: "Starting".into(),
        },
    ];

    feed.update(cx, |feed, cx| {
        feed.sync_history(
            Some("s-stream".into()),
            ChatHistory {
                rows: rows.clone(),
                streaming: Some(3),
                status: TurnStatus::Running,
            },
            cx,
        );
        assert_eq!(feed.all_deliverable_items().map_or(0, |v| v.len()), 1);
        assert!(!feed.has_trace_attention());
    });

    // Pure Assistant text growth bypasses cache rebuild and keeps all cached deliverables.
    if let ChatLine::Assistant { text } = &mut rows[3] {
        text.push_str(" ... more markdown tokens");
    }
    feed.update(cx, |feed, cx| {
        feed.sync_history(
            Some("s-stream".into()),
            ChatHistory {
                rows: rows.clone(),
                streaming: Some(3),
                status: TurnStatus::Running,
            },
            cx,
        );
        assert_eq!(feed.all_deliverable_items().map_or(0, |v| v.len()), 1);
        assert!(!feed.has_trace_attention());
    });

    // Appending a failed verification or tool error marks cache dirty and refreshes immediately.
    rows.push(ChatLine::Verification {
        report_id: "r-fail".into(),
        artifact_path: r"C:\Work\deck.pptx".into(),
        artifact_hash: "h2".into(),
        ok: false,
        issues: vec![flowy_agent::VerificationIssue {
            code: "formula_error".into(),
            field: "Slide 2".into(),
        }],
        semantic_hint: None,
        override_used: false,
        provenance_warnings: Vec::new(),
        source_session_id: None,
    });
    feed.update(cx, |feed, cx| {
        feed.sync_history(
            Some("s-stream".into()),
            ChatHistory {
                rows: rows.clone(),
                streaming: None,
                status: TurnStatus::Completed,
            },
            cx,
        );
        assert!(feed.is_path_failed(r"C:\Work\deck.pptx"));
        assert_eq!(feed.failed_verifications().len(), 1);
        assert!(feed.has_trace_attention());
    });
}
