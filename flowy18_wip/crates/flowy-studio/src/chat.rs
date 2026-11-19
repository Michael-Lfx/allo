use crate::panel::StudioEvent;
use flowy_agent::TurnStatus;
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
            .child(self.feed.clone())
            .child(self.working.clone())
            .when(matches!(self.status, TurnStatus::Completed), |column| {
                column.child(
                    div()
                        .w_full()
                        .max_w(px(760.))
                        .mx_auto()
                        .px(self.theme.space(32.))
                        .child(self.rating_row(cx)),
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

    pub(crate) fn rate_result(
        &mut self,
        stars: u32,
        cx: &mut Context<'_, Self>,
    ) {
        cx.emit(StudioEvent::Rated { stars });
        self.emit_notice(Some("chat.rate_thanks"), cx);
    }
}
