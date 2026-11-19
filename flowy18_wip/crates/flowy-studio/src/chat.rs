use crate::panel::StudioEvent;
use flowy_agent::{InventoryStatus, InventorySummary, TurnStatus};
use flowy_ui as theme;
use gpui::{Context, div, prelude::*, px};

impl crate::panel::StudioPanel {
    pub(crate) fn chat_canvas(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id("chat")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .children(
                self.inventory
                    .as_ref()
                    .map(|task| self.inventory_card(task, cx)),
            )
            .child(self.feed.clone())
            .when(self.inventory.is_none(), |column| {
                column.child(self.working.clone())
            })
            .when(
                self.inventory.is_none()
                    && matches!(self.status, TurnStatus::Completed),
                |column| {
                    column.child(
                        div()
                            .w_full()
                            .max_w(px(760.))
                            .mx_auto()
                            .px(self.theme.space(32.))
                            .child(self.rating_row(cx)),
                    )
                },
            )
            .when(self.inventory.is_none(), |column| {
                column.child(self.composer.clone())
            })
    }

    fn inventory_card(
        &self,
        task: &InventorySummary,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let key = match task.status {
            InventoryStatus::WaitingForFolder => "inventory.waiting",
            InventoryStatus::Running => "chat.status_running",
            InventoryStatus::Paused => "inventory.paused",
            InventoryStatus::NeedsReview => "inventory.needs_review",
            InventoryStatus::Delivered => "chat.status_completed",
            InventoryStatus::Failed => "chat.status_error",
            InventoryStatus::Cancelled => "chat.status_stopped",
        };
        let card = div()
            .id("inventory-detail")
            .w_full()
            .max_w(px(760.))
            .mx_auto()
            .p(self.theme.space(16.))
            .mt(self.theme.space(12.))
            .rounded_xl()
            .border_1()
            .border_color(self.theme.line())
            .bg(self.theme.card())
            .flex()
            .flex_col()
            .gap(self.theme.space(8.))
            .child(div().text_lg().child(task.goal.clone()))
            .child(
                div()
                    .id("inventory-status")
                    .role(gpui::Role::Status)
                    .aria_label(self.t(key))
                    .aria_live(gpui::accesskit::Live::Polite)
                    .text_sm()
                    .text_color(self.theme.muted())
                    .child(self.t(key)),
            )
            .children(task.folder.as_ref().map(|path| {
                div()
                    .text_xs()
                    .text_color(self.theme.muted())
                    .child(path.display().to_string())
            }));
        let action = match task.status {
            InventoryStatus::WaitingForFolder
            | InventoryStatus::Paused
            | InventoryStatus::Failed => div()
                .flex()
                .flex_wrap()
                .gap(self.theme.space(8.))
                .when(task.status != InventoryStatus::WaitingForFolder, |row| {
                    row.child(
                        theme::Button::new(
                            "inventory-resume",
                            self.t("inventory.resume"),
                        )
                        .render(
                            &self.theme,
                            cx.listener(|panel, _, _, cx| {
                                panel.resume_inventory(None, cx);
                            }),
                        ),
                    )
                })
                .child(
                    theme::Button::new(
                        "inventory-pick",
                        self.t("inventory.pick_folder"),
                    )
                    .render(
                        &self.theme,
                        cx.listener(|panel, _, _, cx| {
                            panel.pick_inventory_folder(true, cx);
                        }),
                    ),
                )
                .into_any_element(),
            InventoryStatus::Running => {
                theme::Button::new("inventory-stop", self.t("chat.stop"))
                    .render(
                        &self.theme,
                        cx.listener(|panel, _, _, cx| panel.stop_turn(cx)),
                    )
                    .into_any_element()
            }
            InventoryStatus::Delivered => div()
                .flex()
                .flex_wrap()
                .gap(self.theme.space(8.))
                .child(
                    theme::Button::new(
                        "inventory-open",
                        format!("{} inventory.md", self.t("shell.notify_open")),
                    )
                    .variant(theme::ControlVariant::Primary)
                    .render(
                        &self.theme,
                        cx.listener(|panel, _, _, cx| {
                            panel.launch_inventory_file(false, cx);
                        }),
                    ),
                )
                .child(
                    theme::Button::new(
                        "inventory-reveal",
                        self.t("inventory.reveal"),
                    )
                    .render(
                        &self.theme,
                        cx.listener(|panel, _, _, cx| {
                            panel.launch_inventory_file(true, cx);
                        }),
                    ),
                )
                .into_any_element(),
            InventoryStatus::NeedsReview | InventoryStatus::Cancelled => {
                return card;
            }
        };
        card.child(div().child(action))
    }

    fn launch_inventory_file(
        &mut self,
        reveal: bool,
        cx: &mut Context<'_, Self>,
    ) {
        let path = match self.agent.inventory_artifact() {
            Ok(path) => path,
            Err(key) => {
                self.emit_notice(Some(key), cx);
                return;
            }
        };
        #[cfg(not(windows))]
        if reveal {
            cx.reveal_path(&path);
            return;
        }

        let session = self.agent.active_session_id();
        let notice = if reveal {
            "inventory.reveal_failed"
        } else {
            "inventory.open_failed"
        };
        let launch = cx.background_executor().spawn(async move {
            #[cfg(windows)]
            if reveal {
                return reveal_in_explorer(&path);
            }
            open::that(path)
        });
        cx.spawn(async move |panel, cx| {
            let failed = launch.await.is_err();
            let _ = panel.update(cx, |panel, cx| {
                if panel.agent.active_session_id() == session {
                    if failed {
                        panel.emit_notice(Some(notice), cx);
                    } else if panel.notice == Some(notice) {
                        panel.emit_notice(None, cx);
                    }
                }
            });
        })
        .detach();
    }

    fn rating_row(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut row = div()
            .id("chat-rating")
            .flex()
            .items_center()
            .gap(self.theme.space(8.))
            .pt(self.theme.space(8.))
            .child(
                div()
                    .text_sm()
                    .text_color(self.theme.muted())
                    .child(self.t("chat.rate_prompt")),
            );
        for star in 1..=5 {
            row = row.child(
                theme::interactive(
                    ("chat-star", star),
                    self.tf("ui.rating", &[("n", &star.to_string())]),
                    &self.theme,
                )
                .px(self.theme.space(4.))
                .cursor_pointer()
                .child(theme::icons::star())
                .on_click(cx.listener(
                    move |panel, _, _, cx| {
                        panel.rate_result(star, cx);
                    },
                )),
            );
        }
        row
    }

    pub(crate) fn rate_result(
        &mut self,
        stars: u32,
        cx: &mut Context<'_, Self>,
    ) {
        cx.emit(StudioEvent::Rated { stars });
        self.emit_notice(Some("chat.rate_thanks"), cx);
    }
}

#[cfg(windows)]
fn reveal_in_explorer(path: &std::path::Path) -> std::io::Result<()> {
    use std::ffi::OsString;
    use std::os::windows::process::CommandExt as _;
    use std::path::PathBuf;
    use std::process::Command;

    let explorer = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .ok_or_else(|| {
            std::io::Error::other("Windows directory is unavailable")
        })?
        .join("explorer.exe");
    // Explorer uses its own comma-separated command line. A verified Windows
    // file path cannot contain the quotes that delimit it.
    let mut select = OsString::from("/select,\"");
    select.push(path.as_os_str());
    select.push("\"");
    // Explorer hands the selection to the shell and may then exit nonzero.
    Command::new(explorer).raw_arg(select).spawn().map(|_| ())
}
