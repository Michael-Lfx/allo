use super::{FlowyApp, Section};
use crate::persona::{
    ABOUT_MAX, INSTRUCTIONS_MAX, KNOWLEDGE_MAX, Knowledge, NAME_MAX, Profile,
    limited, read_patch, save,
};
use flowy_ui as theme;
use gpui::{
    AnyElement, ClipboardItem, ColorExt, Context, Entity, div, prelude::*, px,
};
use gpui_ce_elements::editable_text::{
    EditableTextState, text_area, text_input,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Page {
    Computer,
    Skills,
    Personal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tab {
    Profile,
    Knowledge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Dialog {
    None,
    Import,
    Knowledge,
}

const FLYOUT_TOP: f32 = 242.0;
const PASTE_MAX: usize = 8000;

const MENU: &[(&str, &str, Page)] = &[
    ("more-computer", "more.computer", Page::Computer),
    ("more-skills", "more.skills", Page::Skills),
    ("more-personal", "more.personal", Page::Personal),
];

impl FlowyApp {
    pub(super) fn toggle_more(&mut self, cx: &mut Context<'_, Self>) {
        self.studio.update(cx, |panel, cx| {
            panel.model_menu_open = false;
            cx.notify();
        });
        if self.section == Section::More {
            self.persona.menu = !self.persona.menu;
        } else {
            self.section = Section::More;
            self.persona.menu = true;
            self.notice = None;
        }
        self.sync_studio_surface(cx);
        cx.notify();
    }

    pub fn more_flyout(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let mut menu = div()
            .id("more-flyout")
            .occlude()
            .absolute()
            .left(px(theme::DOCK_WIDTH - 16.))
            .top(px(FLYOUT_TOP))
            .w(px(196.))
            .bg(theme::card())
            .border_1()
            .border_color(theme::line())
            .rounded_xl()
            .shadow_md()
            .p_1()
            .flex()
            .flex_col()
            .on_click(cx.listener(|_app, _event, _window, cx| {
                cx.stop_propagation();
            }));
        for (id, key, page) in MENU {
            menu = menu.child(self.more_flyout_row(id, key, *page, cx));
        }
        menu
    }

    pub fn more_canvas(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let mut page = div().id("more-page").size_full().flex().flex_col();
        page = if self.persona.page == Page::Personal {
            page.child(self.personal_canvas(cx))
        } else if self.persona.page == Page::Skills {
            page.child(self.more_note("more.skills", "more.skills_body"))
        } else {
            page.child(self.more_note("more.computer", "more.computer_body"))
        };
        page
    }

    pub(super) fn more_layer(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let mut layer = div()
            .id("more-layer")
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme::ink().opacity(0.28))
            .on_click(cx.listener(|app, _event, _window, cx| {
                cx.stop_propagation();
                app.close_more_dialog(cx);
            }));
        layer = if self.persona.dialog == Dialog::Knowledge {
            layer.child(self.knowledge_dialog(cx))
        } else {
            layer.child(self.import_dialog(cx))
        };
        layer
    }

    pub(super) fn touch_profile(&self, cx: &mut Context<'_, Self>) {
        clamp_text(&self.persona.nickname, NAME_MAX, cx);
        clamp_text(&self.persona.occupation, NAME_MAX, cx);
        clamp_text(&self.persona.about, ABOUT_MAX, cx);
        clamp_text(&self.persona.instructions, INSTRUCTIONS_MAX, cx);
        self.write_profile(cx);
        cx.notify();
    }

    pub(super) fn touch_paste(&self, cx: &mut Context<'_, Self>) {
        clamp_text(&self.persona.import_paste, PASTE_MAX, cx);
        cx.notify();
    }

    pub(super) fn touch_knowledge_name(&self, cx: &mut Context<'_, Self>) {
        clamp_text(&self.persona.knowledge_name, NAME_MAX, cx);
        cx.notify();
    }

    pub(super) fn touch_knowledge_usage(&self, cx: &mut Context<'_, Self>) {
        clamp_text(&self.persona.knowledge_usage, NAME_MAX, cx);
        cx.notify();
    }

    pub(super) fn touch_knowledge_body(&self, cx: &mut Context<'_, Self>) {
        clamp_text(&self.persona.knowledge_body, KNOWLEDGE_MAX, cx);
        cx.notify();
    }

    fn more_flyout_row(
        &self,
        id: &'static str,
        key: &'static str,
        page: Page,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let selected = self.persona.page == page;
        div()
            .id(id)
            .px_2()
            .py_2()
            .rounded_md()
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .hover(|row| row.bg(theme::soft()))
            .on_click(cx.listener(move |app, _event, _window, cx| {
                cx.stop_propagation();
                app.persona.page = page;
                app.section = Section::More;
                app.notice = None;
                cx.notify();
            }))
            .child(menu_icon(page))
            .child(div().flex_1().text_sm().child(self.t(key)))
            .when(selected, |row| row.child(div().text_sm().child("✓")))
    }

    fn more_note(
        &self,
        title_key: &'static str,
        body_key: &'static str,
    ) -> impl IntoElement {
        div().flex_1().px_10().pt_8().child(
            div()
                .w_full()
                .max_w(px(560.))
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .text_2xl()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(self.t(title_key)),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme::muted())
                        .child(self.t(body_key)),
                ),
        )
    }

    fn personal_canvas(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .id("more-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                div()
                    .min_h_full()
                    .px_10()
                    .pt_8()
                    .pb_6()
                    .child(self.personal_column(cx)),
            )
    }

    fn personal_column(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut column = div()
            .w_full()
            .max_w(px(760.))
            .min_h_full()
            .flex()
            .flex_col()
            .gap_5()
            .child(
                div()
                    .text_2xl()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.t("more.personal")),
            )
            .child(self.personal_tabs(cx));
        column = if self.persona.tab == Tab::Profile {
            column
                .child(self.import_card(cx))
                .child(self.profile_fields(cx))
                .child(div().flex_1())
                .child(self.import_footer(cx))
        } else {
            column.child(self.knowledge_pane(cx))
        };
        column
    }

    fn personal_tabs(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .flex()
            .gap_6()
            .border_b_1()
            .border_color(theme::line())
            .child(self.personal_tab(
                "more-tab-profile",
                "more.profile",
                Tab::Profile,
                cx,
            ))
            .child(self.personal_tab(
                "more-tab-knowledge",
                "more.knowledge",
                Tab::Knowledge,
                cx,
            ))
    }

    fn personal_tab(
        &self,
        id: &'static str,
        key: &'static str,
        tab: Tab,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let selected = self.persona.tab == tab;
        div()
            .id(id)
            .pb_2()
            .cursor_pointer()
            .text_sm()
            .text_color(if selected {
                theme::ink()
            } else {
                theme::muted()
            })
            .border_b_2()
            .border_color(if selected {
                theme::ink()
            } else {
                theme::canvas()
            })
            .child(self.t(key))
            .on_click(cx.listener(move |app, _event, _window, cx| {
                app.persona.tab = tab;
                cx.notify();
            }))
    }

    fn import_card(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .id("import-memory-card")
            .w_full()
            .px_4()
            .py_3()
            .rounded_xl()
            .border_1()
            .border_color(theme::line())
            .bg(theme::card())
            .flex()
            .items_center()
            .gap_3()
            .cursor_pointer()
            .hover(|card| card.bg(theme::soft()))
            .on_click(cx.listener(|app, _event, _window, cx| {
                app.open_import(cx);
            }))
            .child(card_mark())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_sm().child(self.t("more.import_card")))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::muted())
                            .whitespace_normal()
                            .child(self.t("more.import_card_body")),
                    ),
            )
            .child(div().text_sm().text_color(theme::muted()).child("›"))
    }

    fn profile_fields(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .gap_4()
                    .child(self.named_field(
                        "more-nickname",
                        "more.nickname",
                        "more.nickname_placeholder",
                        &self.persona.nickname,
                    ))
                    .child(self.named_field(
                        "more-occupation",
                        "more.occupation",
                        "more.occupation_placeholder",
                        &self.persona.occupation,
                    )),
            )
            .child(labeled_area(
                self.t("more.about"),
                "more-about",
                self.persona.about.clone(),
                self.t("more.about_placeholder"),
                self.count_text(
                    self.persona.about.read(cx).as_str(),
                    ABOUT_MAX,
                ),
                120.,
            ))
            .child(
                div()
                    .text_xs()
                    .text_color(theme::muted())
                    .child(self.t("more.about_hint")),
            )
            .child(labeled_area(
                self.t("more.instructions"),
                "more-instructions",
                self.persona.instructions.clone(),
                self.t("more.instructions_placeholder"),
                self.count_text(
                    self.persona.instructions.read(cx).as_str(),
                    INSTRUCTIONS_MAX,
                ),
                96.,
            ))
    }

    fn named_field(
        &self,
        id: &'static str,
        label_key: &'static str,
        placeholder_key: &'static str,
        state: &Entity<EditableTextState>,
    ) -> impl IntoElement {
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(field_label(self.t(label_key)))
            .child(single_line(id, state.clone(), self.t(placeholder_key)))
    }

    fn import_footer(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div().w_full().flex().justify_center().child(
            div()
                .id("open-import-memory")
                .px_4()
                .py_2()
                .rounded_lg()
                .bg(theme::soft())
                .border_1()
                .border_color(theme::line())
                .cursor_pointer()
                .text_sm()
                .child(self.t("more.import"))
                .on_click(cx.listener(|app, _event, _window, cx| {
                    app.open_import(cx);
                })),
        )
    }

    fn knowledge_pane(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut pane = div().flex().flex_col().gap_3().child(
            div().flex().justify_end().child(
                div()
                    .id("open-add-knowledge")
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .bg(theme::fill())
                    .text_color(theme::on_fill())
                    .cursor_pointer()
                    .text_sm()
                    .child(self.t("more.add"))
                    .on_click(cx.listener(|app, _event, _window, cx| {
                        app.open_knowledge_dialog(cx);
                    })),
            ),
        );
        if self.persona.knowledge.is_empty() {
            pane = pane.child(
                div()
                    .text_sm()
                    .text_color(theme::muted())
                    .child(self.t("more.knowledge_empty")),
            );
        }
        for (index, item) in self.persona.knowledge.iter().enumerate() {
            pane = pane.child(self.knowledge_row(index, item, cx));
        }
        pane
    }

    fn knowledge_row(
        &self,
        index: usize,
        item: &Knowledge,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .w_full()
            .px_4()
            .py_3()
            .rounded_xl()
            .border_1()
            .border_color(theme::line())
            .bg(theme::card())
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().text_sm().child(item.name.clone()))
                    .child(
                        div()
                            .id(format!("knowledge-remove-{index}"))
                            .text_xs()
                            .text_color(theme::muted())
                            .cursor_pointer()
                            .child(self.t("more.remove"))
                            .on_click(cx.listener(
                                move |app, _event, _window, cx| {
                                    app.remove_knowledge(index, cx);
                                },
                            )),
                    ),
            )
            .when(!item.usage.is_empty(), |row| {
                row.child(
                    div()
                        .text_xs()
                        .text_color(theme::muted())
                        .child(item.usage.clone()),
                )
            })
            .when(!item.body.is_empty(), |row| {
                row.child(
                    div()
                        .text_sm()
                        .whitespace_normal()
                        .child(item.body.clone()),
                )
            })
    }

    fn import_dialog(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let prompt = self.import_prompt();
        let ready = !self
            .persona
            .import_paste
            .read(cx)
            .as_str()
            .trim()
            .is_empty();
        let mut card = dialog_shell("import-memory-dialog", cx)
            .child(self.dialog_title("more.import_title", cx))
            .child(
                div()
                    .text_sm()
                    .text_color(theme::muted())
                    .whitespace_normal()
                    .child(self.t("more.import_lead")),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(step_heading("1", self.t("more.import_step_copy")))
                    .child(self.copy_button(cx)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme::muted())
                    .whitespace_normal()
                    .child(self.t("more.import_step_copy_body")),
            )
            .child(prompt_box(&prompt));
        if let Some(key) = self.persona.status {
            card = card.child(
                div()
                    .text_xs()
                    .text_color(theme::muted())
                    .child(self.t(key)),
            );
        }
        card.child(step_heading("2", self.t("more.import_step_paste")))
            .child(
                div()
                    .text_xs()
                    .text_color(theme::muted())
                    .whitespace_normal()
                    .child(self.t("more.import_step_paste_body")),
            )
            .child(counted_area(
                "import-paste",
                self.persona.import_paste.clone(),
                self.t("more.import_placeholder"),
                String::new(),
                120.,
            ))
            .child(self.dialog_actions(
                "cancel-import",
                "confirm-import",
                "more.import_action",
                ready,
                cx,
            ))
    }

    fn knowledge_dialog(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let ready = !self
            .persona
            .knowledge_name
            .read(cx)
            .as_str()
            .trim()
            .is_empty();
        let count = self.count_text(
            self.persona.knowledge_body.read(cx).as_str(),
            KNOWLEDGE_MAX,
        );
        dialog_shell("add-knowledge-dialog", cx)
            .child(self.dialog_title("more.add_title", cx))
            .child(field_label(self.t("more.knowledge_name")))
            .child(single_line(
                "knowledge-name",
                self.persona.knowledge_name.clone(),
                self.t("more.knowledge_name_placeholder"),
            ))
            .child(field_label(self.t("more.knowledge_when")))
            .child(single_line(
                "knowledge-usage",
                self.persona.knowledge_usage.clone(),
                self.t("more.knowledge_when_placeholder"),
            ))
            .child(field_label(self.t("more.knowledge_body")))
            .child(counted_area(
                "knowledge-body",
                self.persona.knowledge_body.clone(),
                self.t("more.knowledge_body_placeholder"),
                count,
                140.,
            ))
            .child(self.dialog_actions(
                "cancel-knowledge",
                "save-knowledge",
                "more.save",
                ready,
                cx,
            ))
    }

    fn dialog_title(
        &self,
        key: &'static str,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_start()
            .justify_between()
            .gap_3()
            .child(
                div()
                    .text_lg()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.t(key)),
            )
            .child(
                div()
                    .id("close-more-dialog")
                    .size(px(28.))
                    .rounded_md()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|style| style.bg(theme::soft()))
                    .child("×")
                    .on_click(cx.listener(|app, _event, _window, cx| {
                        cx.stop_propagation();
                        app.close_more_dialog(cx);
                    })),
            )
    }

    fn copy_button(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .id("copy-import-prompt")
            .px_3()
            .py_1()
            .rounded_full()
            .bg(theme::fill())
            .text_color(theme::on_fill())
            .text_xs()
            .cursor_pointer()
            .child(self.t("more.copy"))
            .on_click(cx.listener(|app, _event, _window, cx| {
                cx.stop_propagation();
                app.copy_import_prompt(cx);
            }))
    }

    fn dialog_actions(
        &self,
        cancel_id: &'static str,
        confirm_id: &'static str,
        confirm_key: &'static str,
        ready: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .justify_end()
            .gap_2()
            .child(self.cancel_button(cancel_id, cx))
            .child(self.confirm_button(confirm_id, confirm_key, ready, cx))
    }

    fn cancel_button(
        &self,
        id: &'static str,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .px_4()
            .py_2()
            .rounded_lg()
            .border_1()
            .border_color(theme::line())
            .bg(theme::card())
            .text_sm()
            .cursor_pointer()
            .child(self.t("more.cancel"))
            .on_click(cx.listener(|app, _event, _window, cx| {
                cx.stop_propagation();
                app.close_more_dialog(cx);
            }))
    }

    fn confirm_button(
        &self,
        id: &'static str,
        key: &'static str,
        ready: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let mut button = div()
            .id(id)
            .px_4()
            .py_2()
            .rounded_lg()
            .text_sm()
            .bg(if ready { theme::fill() } else { theme::line() })
            .text_color(if ready {
                theme::on_fill()
            } else {
                theme::muted()
            })
            .child(self.t(key));
        if ready {
            button = button.cursor_pointer().on_click(cx.listener(
                move |app, _event, _window, cx| {
                    cx.stop_propagation();
                    if key == "more.save" {
                        app.commit_knowledge(cx);
                    } else {
                        app.apply_import(cx);
                    }
                },
            ));
        }
        button
    }

    fn open_import(&mut self, cx: &mut Context<'_, Self>) {
        self.invite.open = false;
        self.persona.dialog = Dialog::Import;
        self.persona.status = None;
        self.persona
            .import_paste
            .update(cx, |state, cx| state.emplace("", cx));
        cx.notify();
    }

    fn open_knowledge_dialog(&mut self, cx: &mut Context<'_, Self>) {
        self.invite.open = false;
        self.persona.dialog = Dialog::Knowledge;
        self.persona.status = None;
        self.persona
            .knowledge_name
            .update(cx, |state, cx| state.emplace("", cx));
        self.persona
            .knowledge_usage
            .update(cx, |state, cx| state.emplace("", cx));
        self.persona
            .knowledge_body
            .update(cx, |state, cx| state.emplace("", cx));
        cx.notify();
    }

    fn close_more_dialog(&mut self, cx: &mut Context<'_, Self>) {
        self.persona.dialog = Dialog::None;
        self.persona.status = None;
        cx.notify();
    }

    fn copy_import_prompt(&mut self, cx: &mut Context<'_, Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.import_prompt()));
        self.persona.status = Some("more.copied");
        cx.notify();
    }

    fn apply_import(&mut self, cx: &mut Context<'_, Self>) {
        let text = self
            .persona
            .import_paste
            .read(cx)
            .as_str()
            .trim()
            .to_string();
        if text.is_empty() {
            return;
        }
        let nickname = self.t("more.label_nickname");
        let occupation = self.t("more.label_occupation");
        let about = self.t("more.label_about");
        let instructions = self.t("more.label_instructions");
        let none = self.t("more.none");
        let patch = read_patch(
            &text,
            &nickname,
            &occupation,
            &about,
            &instructions,
            &none,
        );
        let nickname_wrote =
            assign_text(&self.persona.nickname, patch.nickname, NAME_MAX, cx);
        let occupation_wrote = assign_text(
            &self.persona.occupation,
            patch.occupation,
            NAME_MAX,
            cx,
        );
        let about_wrote =
            assign_text(&self.persona.about, patch.about, ABOUT_MAX, cx);
        let instructions_wrote = assign_text(
            &self.persona.instructions,
            patch.instructions,
            INSTRUCTIONS_MAX,
            cx,
        );
        let wrote = nickname_wrote
            || occupation_wrote
            || about_wrote
            || instructions_wrote;
        self.persona
            .import_paste
            .update(cx, |state, cx| state.emplace("", cx));
        self.persona.dialog = Dialog::None;
        self.persona.status = None;
        self.persona.page = Page::Personal;
        self.persona.tab = Tab::Profile;
        self.notice = Some(if wrote {
            "more.import_saved"
        } else {
            "more.import_none"
        });
        cx.notify();
    }

    fn commit_knowledge(&mut self, cx: &mut Context<'_, Self>) {
        let name = limited(
            self.persona.knowledge_name.read(cx).as_str().trim(),
            NAME_MAX,
        );
        if name.is_empty() {
            return;
        }
        let usage = limited(
            self.persona.knowledge_usage.read(cx).as_str().trim(),
            NAME_MAX,
        );
        let body = limited(
            self.persona.knowledge_body.read(cx).as_str(),
            KNOWLEDGE_MAX,
        );
        self.persona.knowledge.push(Knowledge { name, usage, body });
        self.persona
            .knowledge_name
            .update(cx, |state, cx| state.emplace("", cx));
        self.persona
            .knowledge_usage
            .update(cx, |state, cx| state.emplace("", cx));
        self.persona
            .knowledge_body
            .update(cx, |state, cx| state.emplace("", cx));
        self.persona.dialog = Dialog::None;
        self.write_profile(cx);
        self.notice = Some("more.knowledge_saved");
        cx.notify();
    }

    fn remove_knowledge(&mut self, index: usize, cx: &mut Context<'_, Self>) {
        if index < self.persona.knowledge.len() {
            self.persona.knowledge.remove(index);
            self.write_profile(cx);
            cx.notify();
        }
    }

    fn write_profile(&self, cx: &Context<'_, Self>) {
        let profile = Profile {
            nickname: self.persona.nickname.read(cx).as_str().to_string(),
            occupation: self.persona.occupation.read(cx).as_str().to_string(),
            about: self.persona.about.read(cx).as_str().to_string(),
            instructions: self
                .persona
                .instructions
                .read(cx)
                .as_str()
                .to_string(),
            knowledge: self.persona.knowledge.clone(),
        };
        save(self.store.root(), &profile);
    }

    fn import_prompt(&self) -> String {
        let nickname = self.t("more.label_nickname");
        let occupation = self.t("more.label_occupation");
        let about = self.t("more.label_about");
        let instructions = self.t("more.label_instructions");
        let none = self.t("more.none");
        self.tf(
            "more.import_prompt",
            &[
                ("nickname", &nickname),
                ("occupation", &occupation),
                ("about", &about),
                ("instructions", &instructions),
                ("none", &none),
            ],
        )
    }

    fn count_text(&self, value: &str, max: usize) -> String {
        let count = value.chars().count().to_string();
        let max = max.to_string();
        self.tf("more.count", &[("count", &count), ("max", &max)])
    }
}

fn dialog_shell(
    id: &'static str,
    cx: &Context<'_, FlowyApp>,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .w(px(480.))
        .max_h(px(640.))
        .overflow_y_scroll()
        .bg(theme::card())
        .rounded_xl()
        .shadow_md()
        .px_6()
        .pt_4()
        .pb_5()
        .flex()
        .flex_col()
        .gap_3()
        .on_click(cx.listener(|_app, _event, _window, cx| {
            cx.stop_propagation();
        }))
}

fn single_line(
    id: &'static str,
    state: Entity<EditableTextState>,
    placeholder: String,
) -> impl IntoElement {
    text_input(id)
        .state(state.downgrade())
        .placeholder(placeholder)
        .placeholder_color(theme::muted())
        .caret_color(gpui::rgb_to_hsla(theme::ink()))
        .caret_blink_interval_500ms()
        .w_full()
        .min_h_auto()
        .whitespace_nowrap()
        .px_3()
        .py_2()
        .rounded_lg()
        .border_1()
        .border_color(theme::line())
        .bg(theme::soft())
        .text_color(theme::ink())
}

fn counted_area(
    id: &'static str,
    state: Entity<EditableTextState>,
    placeholder: String,
    count: String,
    height: f32,
) -> impl IntoElement {
    let mut area = div()
        .w_full()
        .rounded_lg()
        .border_1()
        .border_color(theme::line())
        .bg(theme::soft())
        .px_3()
        .pt_2()
        .pb_1()
        .flex()
        .flex_col()
        .child(
            text_area(id)
                .state(state.downgrade())
                .placeholder(placeholder)
                .placeholder_color(theme::muted())
                .caret_color(gpui::rgb_to_hsla(theme::ink()))
                .caret_blink_interval_500ms()
                .w_full()
                .h(px(height))
                .whitespace_normal()
                .overflow_y_scroll()
                .text_color(theme::ink()),
        );
    if !count.is_empty() {
        area = area.child(
            div()
                .w_full()
                .flex()
                .justify_end()
                .text_xs()
                .text_color(theme::muted())
                .child(count),
        );
    }
    area
}

fn labeled_area(
    label: String,
    id: &'static str,
    state: Entity<EditableTextState>,
    placeholder: String,
    count: String,
    height: f32,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .child(field_label(label))
        .child(counted_area(id, state, placeholder, count, height))
}

fn field_label(text: String) -> impl IntoElement {
    div().pb_1().text_sm().child(text)
}

fn step_heading(number: &str, title: String) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .size(px(22.))
                .rounded_full()
                .bg(theme::soft())
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .child(number.to_string()),
        )
        .child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(title),
        )
}

fn prompt_box(text: &str) -> impl IntoElement {
    let mut box_ = div()
        .w_full()
        .p_3()
        .rounded_lg()
        .bg(theme::soft())
        .flex()
        .flex_col()
        .gap_1();
    for line in text.lines() {
        let mut row = div().text_xs().text_color(theme::muted());
        row = if line.is_empty() {
            row.h(px(8.))
        } else {
            row.child(line.to_string())
        };
        box_ = box_.child(row);
    }
    box_
}

fn card_mark() -> impl IntoElement {
    div()
        .size(px(28.))
        .rounded_md()
        .bg(theme::soft())
        .flex()
        .items_center()
        .justify_center()
        .child(div().size(px(8.)).rounded_full().bg(theme::ink()))
}

fn menu_icon(page: Page) -> AnyElement {
    match page {
        Page::Computer => monitor_icon().into_any_element(),
        Page::Skills => skill_icon().into_any_element(),
        Page::Personal => sliders_icon().into_any_element(),
    }
}

fn monitor_icon() -> impl IntoElement {
    div()
        .size(px(16.))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(1.))
        .child(
            div()
                .w(px(12.))
                .h(px(8.))
                .rounded(px(2.))
                .border_1()
                .border_color(theme::ink()),
        )
        .child(div().w(px(6.)).h(px(2.)).bg(theme::ink()))
}

fn skill_icon() -> impl IntoElement {
    div()
        .size(px(16.))
        .flex()
        .items_center()
        .justify_center()
        .child(div().size(px(6.)).rounded(px(1.)).bg(theme::ink()))
}

fn sliders_icon() -> impl IntoElement {
    div()
        .size(px(16.))
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(3.))
        .child(div().w(px(12.)).h(px(2.)).bg(theme::ink()))
        .child(div().ml(px(4.)).w(px(8.)).h(px(2.)).bg(theme::ink()))
}

fn assign_text(
    field: &Entity<EditableTextState>,
    value: Option<String>,
    max: usize,
    cx: &mut Context<'_, FlowyApp>,
) -> bool {
    let Some(value) = value else {
        return false;
    };
    let value = limited(&value, max);
    field.update(cx, |state, cx| state.emplace(&value, cx));
    true
}

fn clamp_text(
    field: &Entity<EditableTextState>,
    max: usize,
    cx: &mut Context<'_, FlowyApp>,
) {
    let current = field.read(cx).as_str().to_string();
    let next = limited(&current, max);
    if next != current {
        field.update(cx, |state, cx| state.emplace(&next, cx));
    }
}
