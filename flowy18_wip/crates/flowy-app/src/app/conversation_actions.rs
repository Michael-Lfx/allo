use super::{FlowyApp, Section, state::ActiveDialog};
use flowy_ui as ui;
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, ParentElement,
    Pixels, Point, Task, Window, div, prelude::*, px,
};
use gpui_kit::base::input::InputState;
use std::time::Duration;

const ARCHIVE_UNDO_TIMEOUT: Duration = Duration::from_secs(5);

struct ArchiveUndoEntry {
    session_id: String,
    title: String,
    was_viewing_active: bool,
    _timer: Task<()>,
}

/// Minimal single-slot undo state for the 5-second post-archive banner.
#[derive(Default)]
pub(super) struct ArchiveUndoController {
    generation: u64,
    active: Option<ArchiveUndoEntry>,
}

impl ArchiveUndoController {
    const fn bump(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.generation
    }

    pub(super) fn clear(&mut self) {
        self.bump();
        self.active = None;
    }

    fn clear_if_session(&mut self, session_id: &str) {
        if self
            .active
            .as_ref()
            .is_some_and(|entry| entry.session_id == session_id)
        {
            self.clear();
        }
    }
}

/// Active anchor target for the unified session context menu (shared between
/// sidebar `...` button, sidebar right-click, and Progress Board rows).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct TaskMenuState {
    pub(super) session_id: String,
    pub(super) anchor_id: String,
}

#[derive(Default)]
pub(super) struct ConversationDeleteState {
    id: Option<String>,
    busy: bool,
    error: Option<&'static str>,
    generation: u64,
    pending: Option<DeleteRequest>,
}

pub(super) struct ConversationRenameState {
    pub(super) id: Option<String>,
    pub(super) input: Entity<InputState>,
    pub(super) error: Option<&'static str>,
}

impl ConversationRenameState {
    pub(super) const fn new(input: Entity<InputState>) -> Self {
        Self {
            id: None,
            input,
            error: None,
        }
    }
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
    pub(super) const fn conversation_delete_busy(&self) -> bool {
        self.conversation_delete.busy
    }

    pub(super) fn toggle_task_menu(
        &mut self,
        session_id: &str,
        cx: &mut Context<'_, Self>,
    ) {
        self.toggle_task_menu_anchored(
            session_id,
            format!("task-menu-trigger-{session_id}"),
            cx,
        );
    }

    pub(super) fn toggle_task_menu_anchored(
        &mut self,
        session_id: &str,
        anchor_id: String,
        cx: &mut Context<'_, Self>,
    ) {
        if self.conversation_delete.busy {
            return;
        }
        let id = session_id.to_owned();
        self.user_menu = false;
        self.project_menu = None;
        self.ui
            .overlays
            .update(cx, flowy_ui::OverlayHost::clear_point_anchors);
        let same = self.task_menu.as_ref().is_some_and(|menu| {
            menu.session_id == id && menu.anchor_id == anchor_id
        });
        self.task_menu = (!same).then_some(TaskMenuState {
            session_id: id,
            anchor_id,
        });
        cx.notify();
    }

    /// Opens the unified session menu anchored at a window coordinate (used by
    /// right-click on sidebar rows and Progress Board rows).
    pub(super) fn open_task_menu_at(
        &mut self,
        session_id: &str,
        position: Point<Pixels>,
        cx: &mut Context<'_, Self>,
    ) {
        if self.conversation_delete.busy {
            return;
        }
        let id = session_id.to_owned();
        let anchor_id = format!("task-menu-point-{id}");
        self.user_menu = false;
        self.project_menu = None;
        self.ui.overlays.update(cx, |host, cx| {
            host.set_point_anchor(anchor_id.clone(), position, cx);
        });
        self.task_menu = Some(TaskMenuState {
            session_id: id,
            anchor_id,
        });
        cx.notify();
    }

    pub(super) fn task_menu_content(
        &self,
        session_id: &str,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let session = self
            .studio
            .read(cx)
            .agent
            .list_sessions()
            .into_iter()
            .find(|s| s.id == session_id);
        let pinned = session.as_ref().is_some_and(|s| s.pinned);
        let archived = session.as_ref().is_some_and(|s| s.archived);

        let pin_id = session_id.to_owned();
        let rename_id = session_id.to_owned();
        let archive_id = session_id.to_owned();
        let delete_id = session_id.to_owned();

        let pin_label = self.t(if pinned { "chat.unpin" } else { "chat.pin" });
        let rename_label = self.t("chat.rename");
        let archive_label = self.t(if archived {
            "chat.unarchive"
        } else {
            "chat.archive"
        });
        let delete_label = self.t("chat.delete");
        let width = super::projects::project_menu_width(
            window,
            &[
                pin_label.as_str(),
                rename_label.as_str(),
                archive_label.as_str(),
                delete_label.as_str(),
            ],
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
            "pin-conversation",
            ui::icons::pin(),
            pin_label,
            false,
            cx,
            move |app, cx| app.toggle_session_pinned(&pin_id, !pinned, cx),
        ))
        .child(self.project_menu_row(
            "rename-conversation",
            ui::icons::square_pen(),
            rename_label,
            false,
            cx,
            move |app, cx| app.open_conversation_rename(&rename_id, cx),
        ))
        .child(self.project_menu_row(
            "archive-conversation",
            ui::icons::archive(),
            archive_label,
            false,
            cx,
            move |app, cx| {
                app.toggle_session_archived(&archive_id, !archived, cx);
            },
        ))
        .child(div().my(px(2.)).h(px(1.)).w_full().bg(self.theme.line()))
        .child(self.project_menu_row(
            "delete-conversation",
            ui::icons::trash(),
            delete_label,
            true,
            cx,
            move |app, cx| {
                if !app.conversation_delete.open(delete_id.clone()) {
                    return;
                }
                app.task_menu = None;
                app.activate_dialog(ActiveDialog::ConversationDelete, cx);
            },
        ))
    }

    pub(super) fn toggle_session_pinned(
        &mut self,
        session_id: &str,
        pinned: bool,
        cx: &mut Context<'_, Self>,
    ) {
        self.task_menu = None;
        let result = self
            .studio
            .read(cx)
            .agent
            .set_session_pinned(session_id, pinned);
        if let Err(key) = result {
            self.studio.update(cx, |panel, cx| panel.note(key, cx));
        } else {
            self.studio
                .update(cx, flowy_studio::StudioPanel::refresh_board_now);
        }
        cx.notify();
    }

    pub(super) fn toggle_session_archived(
        &mut self,
        session_id: &str,
        archived: bool,
        cx: &mut Context<'_, Self>,
    ) {
        self.task_menu = None;
        let in_studio = self.section == Section::Studio;
        let (was_active, was_viewing_active, title) = {
            let studio = self.studio.read(cx);
            let was_active =
                studio.agent.active_session_id().as_deref() == Some(session_id);
            let was_viewing_active = in_studio
                && studio.surface == flowy_studio::StudioSurface::Chat
                && was_active;
            let raw_title = studio
                .agent
                .list_sessions()
                .into_iter()
                .find(|s| s.id == session_id)
                .map(|s| s.title.trim().to_owned())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| self.t("dock.new_task"));
            let title = flowy_studio::ports::redact_trace_text(&raw_title);
            (was_active, was_viewing_active, title)
        };
        let result = self
            .studio
            .read(cx)
            .agent
            .set_session_archived(session_id, archived);
        if let Err(key) = result {
            self.studio.update(cx, |panel, cx| panel.note(key, cx));
        } else {
            self.studio.update(cx, |panel, cx| {
                if archived && was_active {
                    if in_studio {
                        panel.new_task(cx);
                    } else {
                        let _ = panel.agent.clear_active();
                    }
                }
                panel.refresh_board_now(cx);
            });
            if archived {
                let generation = self.archive_undo.bump();
                let timer = cx.spawn(async move |weak, cx| {
                    cx.background_executor().timer(ARCHIVE_UNDO_TIMEOUT).await;
                    let _ = weak.update(cx, |app, cx| {
                        if app.archive_undo.generation == generation
                            && app.archive_undo.active.take().is_some()
                        {
                            cx.notify();
                        }
                    });
                });
                self.archive_undo.active = Some(ArchiveUndoEntry {
                    session_id: session_id.to_owned(),
                    title,
                    was_viewing_active,
                    _timer: timer,
                });
            } else {
                self.archive_undo.clear_if_session(session_id);
            }
        }
        cx.notify();
    }

    pub(super) fn undo_last_archive(&mut self, cx: &mut Context<'_, Self>) {
        self.archive_undo.bump();
        let Some(entry) = self.archive_undo.active.take() else {
            return;
        };
        let result = self
            .studio
            .read(cx)
            .agent
            .set_session_archived(&entry.session_id, false);
        if let Err(key) = result {
            self.studio.update(cx, |panel, cx| panel.note(key, cx));
        } else {
            let should_reopen = entry.was_viewing_active
                && self.studio.read(cx).agent.active_session_id().is_none();
            self.studio.update(cx, |panel, cx| {
                if should_reopen {
                    panel.open_session(&entry.session_id, cx);
                }
                panel.refresh_board_now(cx);
            });
            if should_reopen {
                self.push_section(Section::Studio, cx);
            }
        }
        cx.notify();
    }

    pub(super) fn archive_undo_banner(
        &self,
        cx: &Context<'_, Self>,
    ) -> Option<AnyElement> {
        let title = self
            .archive_undo
            .active
            .as_ref()
            .map(|entry| entry.title.as_str())?;
        let message = self.tf("chat.archived_toast", &[("title", title)]);
        let undo_label = self.t("chat.undo");
        let close_label = self.t("ui.close");
        let bottom_offset = if self.prefs.activity { 108. } else { 20. };
        Some(
            div()
                .id("archive-undo-banner")
                .occlude()
                .role(gpui::Role::Status)
                .absolute()
                .bottom(px(bottom_offset))
                .right(px(24.))
                .max_w(px(420.))
                .px(px(14.))
                .py(px(10.))
                .rounded_xl()
                .bg(self.theme.card())
                .border_1()
                .border_color(self.theme.line())
                .shadow_md()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(
                    ui::icons::archive()
                        .size(px(15.))
                        .flex_none()
                        .text_color(self.theme.muted()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .truncate()
                        .text_sm()
                        .text_color(self.theme.ink())
                        .child(message),
                )
                .child(
                    ui::interactive(
                        "archive-undo-action",
                        undo_label.clone(),
                        &self.theme,
                    )
                    .flex_none()
                    .px(px(10.))
                    .py(px(4.))
                    .rounded_lg()
                    .cursor_pointer()
                    .bg(self.theme.status_active_bg())
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(self.theme.status_active())
                    .hover(|s| s.opacity(0.85))
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.undo_last_archive(cx);
                    }))
                    .child(undo_label),
                )
                .child(
                    ui::IconButton::new(
                        "archive-undo-dismiss",
                        close_label,
                        ui::icons::x().size(px(13.)),
                    )
                    .size(ui::ControlSize::Small)
                    .render(
                        &self.theme,
                        cx.listener(|app, _, _, cx| {
                            app.archive_undo.clear();
                            cx.notify();
                        }),
                    ),
                )
                .into_any_element(),
        )
    }

    pub(super) fn open_conversation_rename(
        &mut self,
        session_id: &str,
        cx: &mut Context<'_, Self>,
    ) {
        let current_title = self
            .studio
            .read(cx)
            .agent
            .list_sessions()
            .into_iter()
            .find(|s| s.id == session_id)
            .map_or_default(|s| s.title);
        self.conversation_rename.id = Some(session_id.to_owned());
        self.conversation_rename.error = None;
        self.set_input_text(
            &self.conversation_rename.input,
            &current_title,
            cx,
        );
        self.task_menu = None;
        self.activate_dialog(ActiveDialog::ConversationRename, cx);
    }

    pub(super) fn confirm_conversation_rename(
        &mut self,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(id) = self.conversation_rename.id.clone() else {
            return;
        };
        let draft = self.conversation_rename.input.read(cx).value().to_string();
        match self.studio.read(cx).agent.rename_session(&id, &draft) {
            Ok(()) => {
                self.conversation_rename.id = None;
                self.conversation_rename.error = None;
                self.active_dialog = ActiveDialog::None;
                self.studio
                    .update(cx, flowy_studio::StudioPanel::refresh_board_now);
                self.load_library(cx);
                cx.notify();
            }
            Err(key) => {
                self.conversation_rename.error = Some(key);
                cx.notify();
            }
        }
    }

    pub(super) fn conversation_rename_layer(
        &self,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let frame = self.theme.modal_size(window.viewport_size());
        let value = self.conversation_rename.input.read(cx).value();
        let can_save = !value.trim().is_empty();
        let dialog = ui::dialog(
            "conversation-rename-card",
            self.t("chat.rename_title"),
            &self.theme,
            &self.ui,
            cx,
        )
        .w(px(400.))
        .max_w(frame.width)
        .max_h(frame.height)
        .p_5()
        .flex()
        .flex_col()
        .gap_4()
        .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()))
        .on_action(cx.listener(
            |app, _: &flowy_studio::SubmitComposer, _, cx| {
                app.confirm_conversation_rename(cx);
            },
        ))
        .child(
            ui::Title::new(ui::TitleLevel::Card, self.t("chat.rename_title"))
                .render(&self.theme),
        )
        .child(
            ui::TextField::new(
                "conversation-rename-input",
                self.t("chat.rename_title"),
                self.conversation_rename.input.clone(),
            )
            .placeholder(self.t("chat.rename_placeholder"))
            .render(&self.theme, cx),
        )
        .when_some(self.conversation_rename.error, |dialog, key| {
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
                        "cancel-conversation-rename",
                        self.t("library.cancel"),
                    )
                    .render(
                        &self.theme,
                        cx.listener(|app, _, _, cx| {
                            app.conversation_rename.id = None;
                            app.conversation_rename.error = None;
                            app.active_dialog = ActiveDialog::None;
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    ui::Button::new(
                        "confirm-conversation-rename",
                        self.t("library.save"),
                    )
                    .variant(ui::ControlVariant::Primary)
                    .disabled(!can_save)
                    .render(
                        &self.theme,
                        cx.listener(|app, _, _, cx| {
                            app.confirm_conversation_rename(cx);
                        }),
                    ),
                ),
        );
        ui::modal_layer(
            "conversation-rename-layer",
            &self.theme,
            &self.ui,
            dialog,
        )
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
            self.archive_undo.clear_if_session(&request.id);
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
    use gpui::TestAppContext;
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
        assert!(state.finish(&first, Err("notice.session_storage_failed")));
        assert_eq!(state.error, Some("notice.session_storage_failed"));
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
            });
        });
        cx.update(|cx| {
            app.update(cx, |app, cx| {
                app.finish_conversation_delete(&first, Ok(()), cx);
                assert_eq!(app.active_dialog, ActiveDialog::Settings);
                assert!(!app.conversation_delete.busy);
            });
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
                    Err("notice.session_storage_failed"),
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
            });
        });
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }
}
