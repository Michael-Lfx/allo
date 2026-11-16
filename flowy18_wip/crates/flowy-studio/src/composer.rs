use flowy_agent::{Model, TurnStatus};
use flowy_ui as theme;
use gpui::{ColorExt, Context, SharedString, div, prelude::*, px};
use gpui_ce_elements::editable_text::text_area;

impl crate::panel::StudioPanel {
    /// Cursor-style floating composer: dark card, model/access/mode chips, send/stop.
    pub(crate) fn chat_composer(
        &self,
        running: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let has_draft = !self.prompt.read(cx).as_str().trim().is_empty();
        div()
            .id("chat-composer")
            .px_8()
            .pb_5()
            .pt_2()
            .flex()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(720.))
                    .rounded_xl()
                    .border_1()
                    .border_color(theme::line())
                    .bg(theme::composer())
                    .shadow_md()
                    .px_3()
                    .pt_3()
                    .pb_2()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        text_area("chat-prompt")
                            .state(self.prompt.downgrade())
                            .placeholder(SharedString::from(self.t("chat.do_anything")))
                            .placeholder_color(theme::composer_muted())
                            .caret_color(gpui::rgb_to_hsla(theme::on_composer()))
                            .caret_blink_interval_500ms()
                            .w_full()
                            .min_h(px(44.))
                            .max_h(px(160.))
                            .whitespace_normal()
                            .overflow_y_scroll()
                            .text_color(theme::on_composer()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(self.chat_composer_chip(
                                "composer-model",
                                self.composer_model_label(),
                                true,
                                cx,
                            ))
                            .child(self.chat_composer_chip(
                                "composer-plan",
                                self.composer_plan_label(),
                                false,
                                cx,
                            ))
                            .child(self.chat_composer_chip(
                                "composer-access",
                                self.t("chat.full_access"),
                                false,
                                cx,
                            ))
                            .child(self.chat_composer_chip(
                                "composer-mode",
                                self.t("studio.build"),
                                false,
                                cx,
                            ))
                            .child(div().flex_1())
                            .child(self.composer_submit(running, has_draft, cx)),
                    ),
            )
    }

    fn composer_model_label(&self) -> String {
        self.t(self.model.name_key())
    }

    fn composer_plan_label(&self) -> String {
        match self.model {
            Model::Max => self.t("chat.plan_max"),
            Model::Standard => self.t("chat.plan_standard"),
            Model::Lite => self.t("chat.plan_lite"),
            Model::Flex => self.t("model.flex"),
        }
    }

    fn chat_composer_chip(
        &self,
        id: &'static str,
        label: String,
        primary: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .h(px(28.))
            .px_2()
            .rounded_md()
            .flex()
            .items_center()
            .gap_1()
            .text_xs()
            .text_color(if primary {
                theme::on_composer()
            } else {
                theme::composer_muted()
            })
            .cursor_pointer()
            .hover(|style| style.bg(theme::soft().opacity(0.35)))
            .when(primary, |el| el.child(div().child("⚡")))
            .child(label)
            .on_click(cx.listener(move |panel, _, _, cx| {
                if id == "composer-model" || id == "composer-plan" {
                    panel.model_menu_open = !panel.model_menu_open;
                    cx.notify();
                } else {
                    panel.note("notice.attach_later", cx);
                }
            }))
    }

    fn composer_submit(
        &self,
        running: bool,
        has_draft: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        if running {
            div()
                .id("chat-stop")
                .w(px(30.))
                .h(px(30.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme::soft().opacity(0.55))
                .text_color(theme::on_composer())
                .cursor_pointer()
                .hover(|style| style.bg(theme::danger().opacity(0.35)))
                .child("■")
                .on_click(cx.listener(|panel, _, _, cx| {
                    panel.stop_turn(cx);
                }))
        } else {
            div()
                .id("chat-send")
                .w(px(30.))
                .h(px(30.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(if has_draft {
                    theme::on_composer()
                } else {
                    theme::soft().opacity(0.45)
                })
                .text_color(if has_draft {
                    theme::composer()
                } else {
                    theme::composer_muted()
                })
                .cursor_pointer()
                .child("↑")
                .on_click(cx.listener(|panel, _, _, cx| {
                    panel.send_prompt(cx);
                }))
        }
    }

    pub(crate) fn format_duration_secs(&self, secs: u64) -> String {
        self.tf("chat.duration_secs", &[("n", &secs.max(1).to_string())])
    }

    pub(crate) fn working_label(&self, status: &TurnStatus) -> String {
        match status {
            TurnStatus::Error { message } => message.clone(),
            TurnStatus::Running => {
                let duration = self
                    .agent
                    .working_elapsed_secs()
                    .map(|secs| self.format_duration_secs(secs))
                    .unwrap_or_else(|| self.format_duration_secs(1));
                self.tf("chat.working_for", &[("duration", &duration)])
            }
            other => self.t(other.key()),
        }
    }
}
