use super::{FlowyApp, Section};
use crate::prefs::{Model, Plan};
use crate::theme;
use gpui::{Context, SharedString, Window, div, prelude::*, px, rgb};
use gpui_ce_elements::editable_text::text_area;
use std::time::Duration;

impl FlowyApp {
    pub(super) fn main_canvas(
        &self,
        window: &mut Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let mut canvas = div()
            .size_full()
            .relative()
            .flex()
            .flex_row()
            .bg(theme::canvas())
            .text_color(theme::ink())
            .child(self.dock(window, cx))
            .child(self.workspace(cx));
        if self.persona.menu && self.section == Section::More {
            canvas = canvas.child(self.more_flyout(cx));
        }
        canvas
    }

    fn workspace(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut column = div()
            .id("workspace")
            .flex_1()
            .min_w_0()
            .h_full()
            .relative()
            .flex()
            .flex_col();
        let chrome = self.section != Section::More
            && self.section != Section::Automation;
        if chrome {
            column = column.child(self.top_bar(cx));
        }
        if let Some(notice) = self.notice {
            column = column.child(
                div()
                    .px_8()
                    .py_2()
                    .text_sm()
                    .text_color(theme::muted())
                    .child(self.t(notice)),
            );
        }
        column = if self.section == Section::Studio {
            column.child(self.studio(cx))
        } else if self.section == Section::Agent {
            column.child(self.agent_canvas(cx))
        } else if self.section == Section::More {
            column.child(self.more_canvas(cx))
        } else if self.section == Section::Automation {
            column.child(self.automation_canvas(cx))
        } else {
            column.child(self.section_page())
        };
        if self.studio.model_info_open && !self.studio.model_menu_open && chrome
        {
            column = column.child(self.model_info(cx));
        }
        if self.studio.model_menu_open && chrome {
            column = column.child(self.model_menu(cx));
        }
        if self.studio.credits_hover && chrome {
            column = column.child(self.credit_card(cx));
        }
        column
    }

    fn model_info(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .id("model-info")
            .occlude()
            .absolute()
            .top(px(64.))
            .left(px(24.))
            .w(px(320.))
            .bg(theme::card())
            .border_1()
            .border_color(theme::line())
            .rounded_xl()
            .shadow_md()
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_2()
                    .child(div().text_sm().child("✦"))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .whitespace_normal()
                            .child(self.t("model.title")),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme::muted())
                    .whitespace_normal()
                    .child(self.t("model.body")),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(self.model_got_it(cx))
                    .child(self.model_upgrade(cx)),
            )
    }

    fn model_got_it(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .id("model-got-it")
            .px_3()
            .py_1()
            .rounded_full()
            .bg(theme::soft())
            .text_sm()
            .cursor_pointer()
            .hover(|style| style.bg(theme::line()))
            .child(self.t("model.got_it"))
            .on_click(cx.listener(|app, _event, _window, cx| {
                app.dismiss_model_info(cx);
            }))
    }

    fn model_upgrade(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .id("model-upgrade-now")
            .px_3()
            .py_1()
            .rounded_full()
            .bg(theme::fill())
            .text_color(theme::on_fill())
            .text_sm()
            .cursor_pointer()
            .hover(|style| style.opacity(0.9))
            .child(self.t("model.upgrade"))
            .on_click(cx.listener(|app, _event, _window, cx| {
                app.dismiss_model_info(cx);
                app.note("notice.upgrade_later", cx);
            }))
    }

    fn model_button_label(&self) -> String {
        let mark = if self.studio.model_menu_open {
            "▴"
        } else {
            "▾"
        };
        format!("{}  {mark}", self.t(self.prefs.model.name_key()))
    }

    fn model_menu(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut menu = div()
            .id("model-list")
            .occlude()
            .absolute()
            .top(px(60.))
            .left(px(24.))
            .w(px(400.))
            .bg(theme::card())
            .border_1()
            .border_color(theme::line())
            .rounded_xl()
            .shadow_md()
            .p_2()
            .flex()
            .flex_col()
            .gap_1();
        for model in Model::ALL {
            menu = menu.child(self.model_row(model, cx));
        }
        menu
    }

    fn model_row(
        &self,
        model: Model,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let selected = self.prefs.model == model;
        let id = SharedString::from(format!("model-row-{}", model.name_key()));
        div()
            .id(id)
            .px_3()
            .py_2()
            .rounded_lg()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .cursor_pointer()
            .hover(|row| row.bg(theme::soft()))
            .on_click(cx.listener(move |app, _event, _window, cx| {
                app.choose_model(model, cx);
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(self.t(model.name_key())),
                            )
                            .when(model.is_pro(), |row| {
                                row.child(
                                    div()
                                        .px_1()
                                        .rounded_md()
                                        .bg(theme::line())
                                        .text_xs()
                                        .text_color(theme::muted())
                                        .child(self.t("model.pro")),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme::muted())
                            .whitespace_normal()
                            .child(self.t(model.detail_key())),
                    ),
            )
            .when(selected, |row| row.child(div().text_sm().child("✓")))
    }

    fn choose_model(&mut self, model: Model, cx: &mut Context<'_, Self>) {
        if !model.can_use(self.prefs.plan) {
            let key = if model == Model::Flex {
                "model.flex_later"
            } else {
                "notice.upgrade_later"
            };
            self.note(key, cx);
            return;
        }
        self.prefs.model = model;
        self.prefs.save(self.store.root());
        self.studio.model_menu_open = false;
        self.notice = None;
        cx.notify();
    }

    fn dismiss_model_info(&mut self, cx: &mut Context<'_, Self>) {
        self.prefs.model_info_version =
            crate::prefs::Preferences::app_version().to_string();
        self.prefs.save(self.store.root());
        self.studio.model_info_open = false;
        cx.notify();
    }

    fn top_bar(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .h(px(56.))
            .px_6()
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(theme::line())
            .child(
                div()
                    .id("model-menu")
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .cursor_pointer()
                    .hover(|style| style.bg(theme::soft()))
                    .child(self.model_button_label())
                    .on_click(cx.listener(|app, _event, _window, cx| {
                        app.studio.model_menu_open =
                            !app.studio.model_menu_open;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme::muted())
                            .child(self.t("studio.free_plan")),
                    )
                    .child(
                        div()
                            .id("upgrade")
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .bg(theme::fill())
                            .text_color(theme::on_fill())
                            .text_sm()
                            .cursor_pointer()
                            .child(self.t("studio.upgrade"))
                            .on_click(cx.listener(
                                |app, _event, _window, cx| {
                                    app.note("notice.upgrade_later", cx);
                                },
                            )),
                    )
                    .child(self.credit_pill(cx)),
            )
    }

    fn set_credit_hover(&mut self, hovered: bool, cx: &mut Context<'_, Self>) {
        if hovered {
            self.studio.credits_close = None;
            if !self.studio.credits_hover {
                self.studio.credits_hover = true;
                cx.notify();
            }
            return;
        }
        if !self.studio.credits_hover {
            return;
        }
        self.studio.credits_close = Some(cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            let _ = weak.update(cx, |app, cx| {
                app.studio.credits_hover = false;
                cx.notify();
            });
        }));
    }

    fn credit_pill(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .id("credit-pill")
            .px_3()
            .py_1()
            .rounded_full()
            .bg(theme::soft())
            .flex()
            .items_center()
            .gap_1()
            .cursor_pointer()
            .hover(|pill| pill.bg(theme::line()))
            .on_hover(cx.listener(|app, hovered: &bool, _window, cx| {
                app.set_credit_hover(*hovered, cx);
            }))
            .child(div().text_sm().child("✦"))
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(grouped(self.studio.credits)),
            )
    }

    fn credit_card(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let (free, daily) = credit_split(self.studio.credits);
        let plan = match self.prefs.plan {
            Plan::Free => "credits.plan_free",
            Plan::Paid => "credits.plan_paid",
        };
        let daily_note = grouped(DAILY_GRANT);
        div()
            .id("credit-card")
            .occlude()
            .absolute()
            .top(px(48.))
            .right(px(24.))
            .w(px(280.))
            .p_4()
            .rounded_xl()
            .bg(theme::card())
            .border_1()
            .border_color(theme::line())
            .shadow_md()
            .flex()
            .flex_col()
            .gap_3()
            .on_hover(cx.listener(|app, hovered: &bool, _window, cx| {
                app.set_credit_hover(*hovered, cx);
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.t(plan)),
                    )
                    .child(
                        div()
                            .id("credit-upgrade")
                            .px_4()
                            .py_1()
                            .rounded_lg()
                            .bg(theme::fill())
                            .text_color(theme::on_fill())
                            .text_sm()
                            .cursor_pointer()
                            .child(self.t("studio.upgrade"))
                            .on_click(cx.listener(
                                |app, _event, _window, cx| {
                                    app.note("notice.upgrade_later", cx);
                                },
                            )),
                    ),
            )
            .child(
                div()
                    .border_t_1()
                    .border_dashed()
                    .border_dashed_length(4.)
                    .border_dashed_gap(3.)
                    .border_color(theme::line()),
            )
            .child(credit_row(
                "✦",
                self.t("credits.balance"),
                grouped(free),
                true,
            ))
            .child(credit_sub(self.t("credits.free"), Some(grouped(free))))
            .child(credit_row(
                "▣",
                self.t("credits.daily"),
                grouped(daily),
                true,
            ))
            .child(credit_sub(
                self.tf("credits.daily_note", &[("count", &daily_note)]),
                None,
            ))
            .child(
                div()
                    .id("credit-usage")
                    .pt_1()
                    .text_sm()
                    .cursor_pointer()
                    .hover(|row| row.text_color(theme::muted()))
                    .child(format!("{}  ›", self.t("credits.usage")))
                    .on_click(cx.listener(|app, _event, _window, cx| {
                        app.note("credits.usage_later", cx);
                    })),
            )
    }

    fn studio(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        if self.studio.thread.is_some() {
            return self.thread_canvas(cx).into_any_element();
        }
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("studio-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(self.studio_body(cx)),
            )
            .child(self.composer(cx))
            .into_any_element()
    }

    fn studio_body(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let doc = self.studio.document.document();
        div()
            .w_full()
            .max_w(px(980.))
            .mx_auto()
            .px_8()
            .pt_10()
            .pb_6()
            .flex()
            .flex_col()
            .gap_6()
            .child(self.title_row(cx))
            .child(div().text_xs().text_color(theme::muted()).child(format!(
                "{} · {} nodes",
                doc.title,
                doc.nodes.len()
            )))
            .child(self.feature_row(cx))
            .child(self.gallery_row(cx))
    }

    fn title_row(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .flex()
            .items_end()
            .justify_between()
            .gap_6()
            .child(
                div()
                    .font_family("Georgia")
                    .font_weight(gpui::FontWeight::LIGHT)
                    .text_size(px(56.))
                    .child(self.t("studio.brand")),
            )
            .child(
                div()
                    .id("creative-video")
                    .w(px(220.))
                    .h(px(92.))
                    .rounded_xl()
                    .bg(theme::ink())
                    .text_color(theme::on_ink())
                    .p_4()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap_1()
                    .cursor_pointer()
                    .hover(|style| style.opacity(0.92))
                    .on_click(cx.listener(|app, _event, _window, cx| {
                        app.fill_prompt_key("prompt.creative", cx);
                    }))
                    .child(
                        div().text_sm().child(self.t("studio.creative_title")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x00c8_c8c8))
                            .child(self.t("studio.creative_hint")),
                    ),
            )
    }

    fn feature_row(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .flex()
            .gap_4()
            .child(self.build_card(cx))
            .child(self.create_card(cx))
    }

    fn build_card(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .id("build-card")
            .flex_1()
            .h(px(156.))
            .rounded_xl()
            .bg(theme::card())
            .border_1()
            .border_color(theme::line())
            .p_5()
            .flex()
            .flex_col()
            .justify_between()
            .cursor_pointer()
            .hover(|style| style.bg(theme::soft()))
            .on_click(cx.listener(|app, _event, _window, cx| {
                app.fill_prompt_key("prompt.build", cx);
            }))
            .child(
                div()
                    .text_sm()
                    .text_color(theme::muted())
                    .child(self.t("studio.build")),
            )
            .child(div().text_lg().child(self.t("studio.build_body")))
    }

    fn create_card(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .flex_1()
            .h(px(156.))
            .rounded_xl()
            .bg(theme::card())
            .border_1()
            .border_color(theme::line())
            .flex()
            .overflow_hidden()
            .child(
                div()
                    .id("create-card")
                    .flex_1()
                    .p_5()
                    .flex()
                    .flex_col()
                    .justify_between()
                    .cursor_pointer()
                    .hover(|style| style.bg(theme::soft()))
                    .on_click(cx.listener(|app, _event, _window, cx| {
                        app.fill_prompt_key("prompt.create", cx);
                    }))
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme::muted())
                            .child(self.t("studio.create")),
                    )
                    .child(div().text_lg().child(self.t("studio.create_body"))),
            )
            .child(div().w(px(1.)).h_full().bg(theme::line()))
            .child(
                div()
                    .id("local-files")
                    .flex_1()
                    .p_5()
                    .flex()
                    .flex_col()
                    .justify_between()
                    .cursor_pointer()
                    .hover(|style| style.bg(theme::soft()))
                    .on_click(cx.listener(|app, _event, _window, cx| {
                        app.note("notice.local_later", cx);
                    }))
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme::muted())
                            .child(self.t("studio.local_title")),
                    )
                    .child(div().text_sm().child(self.t("studio.local_body"))),
            )
    }

    fn composer(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .px_8()
            .pb_4()
            .child(
                div()
                    .w_full()
                    .max_w(px(760.))
                    .mx_auto()
                    .bg(theme::card())
                    .border_1()
                    .border_color(theme::line())
                    .rounded_xl()
                    .shadow_sm()
                    .px_4()
                    .pt_3()
                    .pb_2()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(self.prompt_field())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .id("attach")
                                    .text_sm()
                                    .text_color(theme::muted())
                                    .cursor_pointer()
                                    .child(self.t("studio.attach"))
                                    .on_click(cx.listener(
                                        |app, _event, _window, cx| {
                                            app.note("notice.attach_later", cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id("send-prompt")
                                    .px_3()
                                    .py_1()
                                    .rounded_full()
                                    .bg(theme::fill())
                                    .text_color(theme::on_fill())
                                    .text_sm()
                                    .cursor_pointer()
                                    .child(self.t("studio.send"))
                                    .on_click(cx.listener(
                                        |app, _event, _window, cx| {
                                            app.send_prompt(cx);
                                        },
                                    )),
                            ),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .max_w(px(760.))
                    .mx_auto()
                    .pt_2()
                    .text_xs()
                    .text_color(theme::muted())
                    .child(self.t("studio.disclaimer")),
            )
    }

    fn agent_canvas(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let events = self.studio.agent.events();
        let mut feed = div()
            .id("agent-feed")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_2();
        if events.is_empty() {
            feed = feed.child(
                div()
                    .text_sm()
                    .text_color(theme::muted())
                    .child(self.t("section.agent")),
            );
        } else {
            for (index, event) in events.iter().enumerate() {
                let label = match event.role {
                    crate::agent::AgentRole::User => "You",
                    crate::agent::AgentRole::Assistant => "Agent",
                };
                feed = feed.child(
                    div()
                        .id(("agent-event", index))
                        .p_3()
                        .rounded_lg()
                        .bg(theme::soft())
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::muted())
                                .child(label),
                        )
                        .child(
                            div()
                                .text_sm()
                                .whitespace_normal()
                                .child(event.text.clone()),
                        ),
                );
            }
        }
        div()
            .id("agent")
            .flex_1()
            .min_h_0()
            .px_8()
            .py_6()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_2xl().child(self.t("dock.agent")))
            .child(
                div()
                    .text_sm()
                    .text_color(theme::muted())
                    .child(self.t("section.agent")),
            )
            .child(feed)
            .child(self.prompt_field())
            .child(
                div()
                    .id("agent-send")
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .text_sm()
                    .bg(theme::fill())
                    .text_color(theme::on_fill())
                    .cursor_pointer()
                    .on_click(cx.listener(|app, _event, _window, cx| {
                        app.send_prompt(cx);
                    }))
                    .child(self.t("studio.send")),
            )
    }

    fn section_page(&self) -> impl IntoElement {
        div().flex_1().flex().items_center().justify_center().child(
            div()
                .w(px(460.))
                .bg(theme::card())
                .border_1()
                .border_color(theme::line())
                .rounded_xl()
                .p_8()
                .flex()
                .flex_col()
                .gap_3()
                .child(div().text_2xl().child(self.t(self.section.title_key())))
                .child(
                    div()
                        .text_sm()
                        .text_color(theme::muted())
                        .child(self.t(self.section.detail_key())),
                ),
        )
    }

    fn prompt_field(&self) -> impl gpui::IntoElement {
        text_area("prompt")
            .state(self.studio.prompt.downgrade())
            .placeholder(self.t("studio.prompt_placeholder"))
            .placeholder_color(theme::muted())
            .caret_color(gpui::rgb_to_hsla(theme::ink()))
            .caret_blink_interval_500ms()
            .w_full()
            .h(px(72.))
            .whitespace_normal()
            .overflow_y_scroll()
            .text_color(theme::ink())
    }
}

const DAILY_GRANT: u32 = 300;

fn credit_split(total: u32) -> (u32, u32) {
    let daily = DAILY_GRANT.min(total);
    (total - daily, daily)
}

fn credit_row(
    mark: &'static str,
    label: String,
    value: String,
    help: bool,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .flex_1()
                .flex()
                .items_center()
                .gap_2()
                .child(div().text_sm().child(mark))
                .child(div().text_sm().child(label))
                .when(help, |row| row.child(help_mark())),
        )
        .child(div().text_sm().child(value))
}

fn credit_sub(label: String, value: Option<String>) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .pl(px(22.))
        .child(div().text_xs().text_color(theme::muted()).child(label))
        .children(value.map(|text| div().text_xs().child(text)))
}

fn help_mark() -> impl IntoElement {
    div()
        .size(px(14.))
        .rounded_full()
        .border_1()
        .border_color(theme::line())
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .text_color(theme::muted())
        .child("?")
}

fn grouped(value: u32) -> String {
    let raw = value.to_string();
    let mut out = String::new();
    for (index, ch) in raw.chars().rev().enumerate() {
        if index > 0 && index.is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::credit_split;

    #[test]
    fn credit_split_keeps_the_total() {
        let (free, daily) = credit_split(2500);
        assert_eq!(free + daily, 2500);
        assert_eq!(daily, 300);
        assert_eq!(free, 2200);
        let (free, daily) = credit_split(100);
        assert_eq!(free, 0);
        assert_eq!(daily, 100);
    }
}
