use crate::panel::StudioEvent;
use flowy_agent::TurnStatus;
use flowy_ui as theme;
use gpui::{Context, div, prelude::*};

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
            .child(self.feed.clone())
            .child(self.working.clone())
            .when(matches!(self.status, TurnStatus::Completed), |column| {
                column.child(
                    div()
                        .px(self.theme.space(32.))
                        .child(self.rating_row(cx))
                        .child(self.suggestions(cx)),
                )
            })
            .child(self.composer.clone())
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
