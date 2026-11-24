use super::{FlowyApp, state::ActiveDialog};
use flowy_ui as ui;
use gpui::{Context, IntoElement, ParentElement, Window, div, prelude::*, px};

#[derive(Default)]
pub(super) struct ConversationDeleteState {
    id: Option<String>,
    busy: bool,
    error: Option<&'static str>,
    generation: u64,
    pending: Option<DeleteRequest>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DeleteRequest {
    id: String,
    generation: u64,
}

impl ConversationDeleteState {
    fn open(&mut self, id: String) -> bool {
        if self.busy {
            return false;
        }
        self.generation = self.generation.wrapping_add(1);
        self.id = Some(id);
        self.error = None;
        true
    }

    fn begin(&mut self) -> Option<DeleteRequest> {
        if self.busy {
            return None;
        }
        let id = self.id.clone()?;
        self.generation = self.generation.wrapping_add(1);
        let request = DeleteRequest {
            id,
            generation: self.generation,
        };
        self.pending = Some(request.clone());
        self.busy = true;
        self.error = None;
        Some(request)
    }

    fn finish(
        &mut self,
        request: &DeleteRequest,
        result: Result<(), &'static str>,
    ) -> bool {
        if self.pending.as_ref() != Some(request)
            || self.id.as_deref() != Some(request.id.as_str())
        {
            return false;
        }
        self.pending = None;
        self.busy = false;
        self.error = result.err();
        if result.is_ok() {
            self.id = None;
        }
        true
    }
}

impl FlowyApp {
    pub(super) fn conversation_delete_busy(&self) -> bool {
        self.conversation_delete.busy
    }

    pub(super) fn toggle_task_menu(
        &mut self,
        session_id: &str,
        cx: &mut Context<'_, Self>,
    ) {
        if self.conversation_delete.busy {
            return;
        }
        let id = session_id.to_owned();
        self.user_menu = false;
        self.project_menu = None;
        self.task_menu =
            (self.task_menu.as_deref() != Some(id.as_str())).then_some(id);
        cx.notify();
    }

    pub(super) fn task_menu_content(
        &self,
        session_id: &str,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let id = session_id.to_owned();
        let label = self.t("chat.delete");
        let width = super::projects::project_menu_width(
            window,
            &[label.as_str()],
            self.theme.body_size(),
        );
        ui::menu(
            "conversation-menu",
            self.t("library.more"),
            &self.theme,
            &self.ui,
            cx,
        )
        .w(width)
        .p_1()
        .gap_1()
        .on_click(
            cx.listener(|_app, _event, _window, cx| cx.stop_propagation()),
        )
        .child(self.project_menu_row(
            "delete-conversation",
            ui::icons::trash(),
            label,
            true,
            cx,
            move |app, cx| {
                if !app.conversation_delete.open(id.clone()) {
                    return;
                }
                app.task_menu = None;
                app.activate_dialog(ActiveDialog::ConversationDelete, cx);
            },
        ))
    }

    pub(super) fn conversation_delete_layer(
        &self,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let frame = self.theme.modal_size(window.viewport_size());
        let title = self
            .conversation_delete
            .id
            .as_ref()
            .and_then(|id| {
                self.studio
                    .read(cx)
                    .agent
                    .list_sessions()
                    .into_iter()
                    .find(|session| &session.id == id)
            })
            .map_or_default(|session| session.title);
        let dialog = ui::dialog(
            "conversation-delete-card",
            self.t("chat.delete_confirm.title"),
            &self.theme,
            &self.ui,
            cx,
        )
        .w(px(420.))
        .max_w(frame.width)
        .max_h(frame.height)
        .p_5()
        .flex()
        .flex_col()
        .gap_4()
        .child(
            ui::Title::new(
                ui::TitleLevel::Card,
                self.t("chat.delete_confirm.title"),
            )
            .render(&self.theme),
        )
        .when(!title.is_empty(), |dialog| {
            dialog.child(
                ui::Text::new(ui::TextRole::Body, title)
                    .render(&self.theme)
                    .whitespace_normal(),
            )
        })
        .child(
            ui::Text::new(
                ui::TextRole::Body,
                self.t("chat.delete_confirm.body"),
            )
            .render(&self.theme)
            .whitespace_normal(),
        )
        .when_some(self.conversation_delete.error, |dialog, key| {
            dialog.child(
                ui::Text::new(ui::TextRole::Body, self.t(key))
                    .render(&self.theme)
                    .text_color(self.theme.danger())
                    .whitespace_normal(),
            )
        })
        .child(
            div()
                .flex()
                .justify_end()
                .gap_2()
                .child(
                    ui::Button::new(
                        "cancel-conversation-delete",
                        self.t("library.cancel"),
                    )
                    .disabled(self.conversation_delete.busy)
                    .render(
                        &self.theme,
                        cx.listener(|app, _, _, cx| {
                            app.active_dialog = ActiveDialog::None;
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    ui::Button::new(
                        "confirm-conversation-delete",
                        self.t("chat.delete"),
                    )
                    .variant(ui::ControlVariant::Danger)
                    .disabled(self.conversation_delete.busy)
                    .render(
                        &self.theme,
                        cx.listener(|app, _, _, cx| {
                            app.confirm_conversation_delete(cx);
                        }),
                    ),
                ),
        );
        ui::modal_layer(
            "conversation-delete-layer",
            &self.theme,
            &self.ui,
            dialog,
        )
    }

    fn confirm_conversation_delete(&mut self, cx: &mut Context<'_, Self>) {
        let Some(request) = self.conversation_delete.begin() else {
            return;
        };
        let agent = self.studio.read(cx).agent.clone();
        let id = request.id.clone();
        let job = cx
            .background_executor()
            .spawn(async move { agent.delete_session(&id) });
        cx.spawn(async move |weak, cx| {
            let result = job.await;
            let _ = weak.update(cx, |app, cx| {
                app.finish_conversation_delete(&request, result, cx);
            });
        })
        .detach();
        cx.notify();
    }

    /// A completion may arrive after Escape closed the confirmation or a
    /// different dialog opened. Only its original request owns delete state.
    fn finish_conversation_delete(
        &mut self,
        request: &DeleteRequest,
        result: Result<(), &'static str>,
        cx: &mut Context<'_, Self>,
    ) {
        let current = self.conversation_delete.finish(request, result);
        if result.is_ok() {
            if current && self.active_dialog == ActiveDialog::ConversationDelete
            {
                self.active_dialog = ActiveDialog::None;
            }
            // The host deletion already succeeded even when its confirmation
            // is no longer current. Refresh global source/catalog projections.
            self.studio.update(cx, |panel, cx| {
                panel.refresh_sessions_after_delete(cx);
            });
            self.load_library(cx);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{ConversationDeleteState, FlowyApp};
    use crate::app::state::ActiveDialog;
    use crate::session::SessionStore;
    use gpui::{TestAppContext, prelude::*};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn test_root() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("build-test")
            .join(format!(
                "conversation-delete-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn pending_delete_owns_its_target_and_rejects_duplicate_or_stale_results() {
        let mut state = ConversationDeleteState::default();
        assert!(state.open("first".into()));
        let first = state.begin().unwrap();
        assert!(!state.open("second".into()));
        assert!(state.begin().is_none());
        assert_eq!(state.id.as_deref(), Some("first"));
        assert!(state.finish(&first, Err("notice.storage_error")));
        assert_eq!(state.error, Some("notice.storage_error"));
        assert!(state.open("second".into()));
        let second = state.begin().unwrap();
        assert!(!state.finish(&first, Ok(())));
        assert!(state.busy);
        assert_eq!(state.id.as_deref(), Some("second"));
        assert_eq!(state.error, None);
        assert!(state.finish(&second, Ok(())));
        assert!(!state.busy);
        assert_eq!(state.id, None);
    }

    #[gpui::test]
    #[expect(
        clippy::needless_pass_by_ref_mut,
        reason = "GPUI test harness passes &mut TestAppContext"
    )]
    fn delayed_delete_completion_preserves_other_dialog_and_new_request(
        cx: &mut TestAppContext,
    ) {
        let root = test_root();
        let (app, _cx) = cx.add_window_view(|window, cx| {
            FlowyApp::new(SessionStore::new(root.clone()), false, window, cx)
        });
        // Hold the service completion across UI turns, as a background delete
        // that has not yet answered would do. Production completion uses the
        // same finish_conversation_delete seam below.
        let first = cx.update(|cx| {
            app.update(cx, |app, _| {
                assert!(app.conversation_delete.open("first".into()));
                app.active_dialog = ActiveDialog::ConversationDelete;
                app.conversation_delete.begin().unwrap()
            })
        });
        cx.update(|cx| {
            app.update(cx, |app, _| {
                // Escape can close a busy confirmation. Opening another delete is
                // refused until the confirmed host operation finishes.
                app.active_dialog = ActiveDialog::None;
                assert!(!app.conversation_delete.open("second".into()));
                app.active_dialog = ActiveDialog::Settings;
            })
        });
        cx.update(|cx| {
            app.update(cx, |app, cx| {
                app.finish_conversation_delete(&first, Ok(()), cx);
                assert_eq!(app.active_dialog, ActiveDialog::Settings);
                assert!(!app.conversation_delete.busy);
            })
        });
        let second = cx.update(|cx| {
            app.update(cx, |app, _| {
                assert!(app.conversation_delete.open("second".into()));
                app.active_dialog = ActiveDialog::ConversationDelete;
                app.conversation_delete.begin().unwrap()
            })
        });
        cx.update(|cx| {
            app.update(cx, |app, cx| {
                // A duplicate late error from the old target cannot poison or
                // close the newly confirmed request.
                app.finish_conversation_delete(
                    &first,
                    Err("notice.storage_error"),
                    cx,
                );
                assert_eq!(app.active_dialog, ActiveDialog::ConversationDelete);
                assert_eq!(
                    app.conversation_delete.id.as_deref(),
                    Some("second")
                );
                assert!(app.conversation_delete.busy);
                assert_eq!(app.conversation_delete.error, None);
                app.finish_conversation_delete(
                    &second,
                    Err("notice.agent_busy"),
                    cx,
                );
                assert_eq!(
                    app.conversation_delete.error,
                    Some("notice.agent_busy")
                );
                assert!(!app.conversation_delete.busy);
            })
        });
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }
}
