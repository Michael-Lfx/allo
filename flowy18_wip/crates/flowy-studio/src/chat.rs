use crate::panel::StudioEvent;
use flowy_agent::{InventoryStatus, InventorySummary, TurnStatus};
use flowy_ui as theme;
use gpui::{Context, div, prelude::*, px};

const SUGGESTIONS: &[&str] =
    &["gallery.documentary", "gallery.budget", "gallery.fashion"];

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
                            .px(self.theme.space(32.))
                            .child(self.rating_row(cx))
                            .child(self.suggestions(cx)),
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
            InventoryStatus::WaitingForFolder => theme::Button::new(
                "inventory-pick",
                self.t("inventory.pick_folder"),
            )
            .render(
                &self.theme,
                cx.listener(|panel, _, _, cx| {
                    panel.pick_inventory_folder(true, cx);
                }),
            )
            .into_any_element(),
            InventoryStatus::Paused | InventoryStatus::Failed => {
                theme::Button::new(
                    "inventory-resume",
                    self.t("inventory.resume"),
                )
                .render(
                    &self.theme,
                    cx.listener(|panel, _, _, cx| {
                        panel.resume_inventory(None, cx);
                    }),
                )
                .into_any_element()
            }
            InventoryStatus::Running => {
                theme::Button::new("inventory-stop", self.t("chat.stop"))
                    .render(
                        &self.theme,
                        cx.listener(|panel, _, _, cx| panel.stop_turn(cx)),
                    )
                    .into_any_element()
            }
            InventoryStatus::Delivered => theme::Button::new(
                "inventory-open",
                format!("{} inventory.md", self.t("shell.notify_open")),
            )
            .variant(theme::ControlVariant::Primary)
            .render(
                &self.theme,
                cx.listener(|panel, _, _, cx| {
                    match panel.agent.inventory_artifact() {
                        Ok(path) => cx.open_with_system(&path),
                        Err(key) => panel.emit_notice(Some(key), cx),
                    }
                }),
            )
            .into_any_element(),
            InventoryStatus::NeedsReview | InventoryStatus::Cancelled => {
                return card;
            }
        };
        card.child(div().child(action))
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

    fn suggestions(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut column = div()
            .id("chat-suggestions")
            .flex()
            .flex_col()
            .gap(self.theme.space(8.))
            .pt(self.theme.space(8.));
        for (index, key) in SUGGESTIONS.iter().enumerate() {
            let key = *key;
            let label = self.t(key);
            column = column.child(
                theme::interactive(
                    ("chat-suggest", index),
                    label.clone(),
                    &self.theme,
                )
                .px(self.theme.space(12.))
                .py(self.theme.space(8.))
                .rounded_lg()
                .bg(self.theme.soft())
                .text_sm()
                .cursor_pointer()
                .child(label)
                .on_click(cx.listener(
                    move |panel, _, _, cx| {
                        panel.fill_prompt_key(key, cx);
                    },
                )),
            );
        }
        column
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
