use super::{FlowyApp, Section};
use crate::persona::{
    ABOUT_MAX, INSTRUCTIONS_MAX, KNOWLEDGE_MAX, Knowledge, NAME_MAX, Profile,
    limited, read_patch, save,
};
use flowy_ui as theme;
use gpui::{AnyElement, ClipboardItem, Context, Entity, div, prelude::*, px};
use gpui_ce_elements::editable_text::EditableTextState;

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
        self.user_menu = false;
        self.persona.menu = !self.persona.menu;
        cx.notify();
    }

    pub fn more_flyout(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut menu = div()
            .id("more-flyout")
            .occlude()
            .w(px(196.))
            .bg(self.theme.card())
            .border_1()
            .border_color(self.theme.line())
            .rounded(self.theme.modal_radius())
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
        window: &mut gpui::Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let mut page = div()
            .id("more-page")
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col();
        page = if self.persona.page == Page::Personal {
            page.child(self.personal_canvas(window, cx))
        } else if self.persona.page == Page::Skills {
            page.child(self.skills_canvas(window, cx))
        } else {
            page.child(
                theme::PageHeader::new(self.t("more.computer"))
                    .render(&self.theme, window),
            )
            .child(self.more_note("more.computer_body"))
        };
        page
    }

    pub(super) fn more_layer(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let content =
            if self.active_dialog == super::state::ActiveDialog::Knowledge {
                self.knowledge_dialog(cx).into_any_element()
            } else {
                self.import_dialog(cx).into_any_element()
            };
        theme::modal_layer("more-layer", &self.theme, &self.ui, content)
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
        theme::interactive(id, self.t(key), &self.theme)
            .px_2()
            .py_2()
            .rounded_md()
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .hover(|row| row.bg(self.theme.soft()))
            .on_click(cx.listener(move |app, _event, _window, cx| {
                cx.stop_propagation();
                app.persona.menu = false;
                app.persona.page = page;
                app.notice = None;
                if app.section == Section::More {
                    app.sync_studio_surface(cx);
                } else {
                    app.push_section(Section::More, cx);
                }
                cx.notify();
            }))
            .child(menu_icon(page))
            .child(
                div()
                    .flex_1()
                    .text_size(self.theme.body_size())
                    .child(self.t(key)),
            )
            .when(selected, |row| row.child(theme::icons::check()))
    }

    fn more_note(&self, body_key: &'static str) -> impl IntoElement {
        div()
            .id("computer-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                div()
                    .w_full()
                    .max_w(px(560.))
                    .px_10()
                    .pt_8()
                    .text_size(self.theme.body_size())
                    .text_color(self.theme.muted())
                    .child(self.t(body_key)),
            )
    }

    fn personal_canvas(
        &self,
        window: &gpui::Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                theme::PageHeader::new(self.t("more.personal"))
                    .render(&self.theme, window),
            )
            .child(
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
                    ),
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
            .border_color(self.theme.line())
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
        theme::interactive(id, self.t(key), &self.theme)
            .role(gpui::Role::Tab)
            .aria_selected(selected)
            .pb_2()
            .cursor_pointer()
            .text_size(self.theme.body_size())
            .text_color(if selected {
                self.theme.ink()
            } else {
                self.theme.muted()
            })
            .border_0()
            .border_b_2()
            .border_color(if selected {
                self.theme.ink()
            } else {
                self.theme.canvas()
            })
            .child(self.t(key))
            .on_click(cx.listener(move |app, _event, _window, cx| {
                app.persona.tab = tab;
                cx.notify();
            }))
    }

    fn import_card(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        theme::interactive(
            "import-memory-card",
            self.t("more.import_card"),
            &self.theme,
        )
        .w_full()
        .px_4()
        .py_3()
        .rounded_xl()
        .border_1()
        .border_color(self.theme.line())
        .bg(self.theme.card())
        .flex()
        .items_center()
        .gap_3()
        .cursor_pointer()
        .hover(|card| card.bg(self.theme.soft()))
        .on_click(cx.listener(|app, _event, _window, cx| {
            app.open_import(cx);
        }))
        .child(card_mark(&self.theme))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_size(self.theme.body_size())
                        .child(self.t("more.import_card")),
                )
                .child(
                    div()
                        .text_size(self.theme.caption_size())
                        .text_color(self.theme.muted())
                        .whitespace_normal()
                        .child(self.t("more.import_card_body")),
                ),
        )
        .child(theme::icons::chevron_right().text_color(self.theme.muted()))
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
                        cx,
                    ))
                    .child(self.named_field(
                        "more-occupation",
                        "more.occupation",
                        "more.occupation_placeholder",
                        &self.persona.occupation,
                        cx,
                    )),
            )
            .child(labeled_area(
                self,
                cx,
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
                    .text_size(self.theme.caption_size())
                    .text_color(self.theme.muted())
                    .child(self.t("more.about_hint")),
            )
            .child(labeled_area(
                self,
                cx,
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
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(field_label(self.t(label_key)))
            .child(single_line(
                cx,
                &self.theme,
                id,
                state.clone(),
                self.t(placeholder_key),
            ))
    }

    fn import_footer(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div().w_full().flex().justify_center().child(
            theme::Button::new("open-import-memory", self.t("more.import"))
                .variant(theme::ControlVariant::Secondary)
                .render(
                    &self.theme,
                    cx.listener(|app, _event, _window, cx| {
                        app.open_import(cx);
                    }),
                ),
        )
    }

    fn knowledge_pane(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut pane = div().flex().flex_col().gap_3().child(
            div().flex().justify_end().child(
                theme::Button::new("open-add-knowledge", self.t("more.add"))
                    .variant(theme::ControlVariant::Primary)
                    .icon(theme::icons::plus())
                    .render(
                        &self.theme,
                        cx.listener(|app, _event, _window, cx| {
                            app.open_knowledge_dialog(cx);
                        }),
                    ),
            ),
        );
        if self.persona.knowledge.is_empty() {
            pane = pane.child(
                div()
                    .text_size(self.theme.body_size())
                    .text_color(self.theme.muted())
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
            .border_color(self.theme.line())
            .bg(self.theme.card())
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
                            .flex_1()
                            .text_size(self.theme.body_size())
                            .child(item.name.clone()),
                    )
                    .child(
                        theme::Button::new(
                            format!("knowledge-remove-{index}"),
                            self.t("more.remove"),
                        )
                        .variant(theme::ControlVariant::Danger)
                        .size(theme::ControlSize::Small)
                        .render(
                            &self.theme,
                            cx.listener(move |app, _event, _window, cx| {
                                app.remove_knowledge(index, cx);
                            }),
                        ),
                    ),
            )
            .when(!item.usage.is_empty(), |row| {
                row.child(
                    div()
                        .text_size(self.theme.caption_size())
                        .text_color(self.theme.muted())
                        .child(item.usage.clone()),
                )
            })
            .when(!item.body.is_empty(), |row| {
                row.child(
                    div()
                        .text_size(self.theme.body_size())
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
        let mut card = dialog_shell(self, "import-memory-dialog", cx)
            .child(self.dialog_title("more.import_title", cx))
            .child(
                div()
                    .text_size(self.theme.body_size())
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(self.t("more.import_lead")),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(step_heading(
                        &self.theme,
                        "1",
                        self.t("more.import_step_copy"),
                    ))
                    .child(self.copy_button(cx)),
            )
            .child(
                div()
                    .text_size(self.theme.caption_size())
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(self.t("more.import_step_copy_body")),
            )
            .child(prompt_box(&self.theme, &prompt));
        if let Some(key) = self.persona.status {
            card = card.child(
                div()
                    .text_size(self.theme.caption_size())
                    .text_color(self.theme.muted())
                    .child(self.t(key)),
            );
        }
        card.child(step_heading(
            &self.theme,
            "2",
            self.t("more.import_step_paste"),
        ))
        .child(
            div()
                .text_size(self.theme.caption_size())
                .text_color(self.theme.muted())
                .whitespace_normal()
                .child(self.t("more.import_step_paste_body")),
        )
        .child(counted_area(
            cx,
            &self.theme,
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
        dialog_shell(self, "add-knowledge-dialog", cx)
            .child(self.dialog_title("more.add_title", cx))
            .child(field_label(self.t("more.knowledge_name")))
            .child(single_line(
                cx,
                &self.theme,
                "knowledge-name",
                self.persona.knowledge_name.clone(),
                self.t("more.knowledge_name_placeholder"),
            ))
            .child(field_label(self.t("more.knowledge_when")))
            .child(single_line(
                cx,
                &self.theme,
                "knowledge-usage",
                self.persona.knowledge_usage.clone(),
                self.t("more.knowledge_when_placeholder"),
            ))
            .child(field_label(self.t("more.knowledge_body")))
            .child(counted_area(
                cx,
                &self.theme,
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
                    .text_size(self.theme.heading_size())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.t(key)),
            )
            .child(
                theme::IconButton::new(
                    "close-more-dialog",
                    self.t("ui.close"),
                    theme::icons::x(),
                )
                .variant(theme::ControlVariant::Ghost)
                .size(theme::ControlSize::Small)
                .render(
                    &self.theme,
                    cx.listener(|app, _event, _window, cx| {
                        cx.stop_propagation();
                        app.close_more_dialog(cx);
                    }),
                ),
            )
    }

    fn copy_button(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        theme::Button::new("copy-import-prompt", self.t("more.copy"))
            .variant(theme::ControlVariant::Ghost)
            .size(theme::ControlSize::Small)
            .render(
                &self.theme,
                cx.listener(|app, _event, _window, cx| {
                    cx.stop_propagation();
                    app.copy_import_prompt(cx);
                }),
            )
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
        theme::Button::new(id, self.t("more.cancel"))
            .variant(theme::ControlVariant::Secondary)
            .render(
                &self.theme,
                cx.listener(|app, _event, _window, cx| {
                    cx.stop_propagation();
                    app.close_more_dialog(cx);
                }),
            )
    }

    fn confirm_button(
        &self,
        id: &'static str,
        key: &'static str,
        ready: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        theme::Button::new(id, self.t(key))
            .variant(theme::ControlVariant::Primary)
            .disabled(!ready)
            .render(
                &self.theme,
                cx.listener(move |app, _, _, cx| {
                    cx.stop_propagation();
                    if key == "more.save" {
                        app.commit_knowledge(cx);
                    } else {
                        app.apply_import(cx);
                    }
                }),
            )
    }

    fn open_import(&mut self, cx: &mut Context<'_, Self>) {
        self.activate_dialog(super::state::ActiveDialog::Import, cx);
        self.persona.status = None;
        self.persona
            .import_paste
            .update(cx, |state, cx| state.emplace("", cx));
        cx.notify();
    }

    fn open_knowledge_dialog(&mut self, cx: &mut Context<'_, Self>) {
        self.activate_dialog(super::state::ActiveDialog::Knowledge, cx);
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
        self.active_dialog = super::state::ActiveDialog::None;
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
        self.active_dialog = super::state::ActiveDialog::None;
        self.persona.status = None;
        self.persona.page = Page::Personal;
        self.persona.tab = Tab::Profile;
        self.notice = Some(if wrote {
            "more.import_saved"
        } else {
            "more.import_none"
        });
        self.sync_studio_surface(cx);
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
        self.active_dialog = super::state::ActiveDialog::None;
        self.write_profile(cx);
        self.notice = Some("more.knowledge_saved");
        self.sync_studio_surface(cx);
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
    app: &FlowyApp,
    id: &'static str,
    cx: &Context<'_, FlowyApp>,
) -> gpui::Stateful<gpui::Div> {
    let theme = &app.theme;
    theme::dialog(id, app.t("more.personal"), theme, &app.ui, cx)
        .w(px(480.))
        .max_h(px(640.))
        .overflow_y_scroll()
        .bg(theme.card())
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
    cx: &Context<'_, FlowyApp>,
    theme: &flowy_ui::ResolvedTheme,
    id: &'static str,
    state: Entity<EditableTextState>,
    placeholder: String,
) -> impl IntoElement {
    theme::TextField::new(id, placeholder.clone(), state)
        .placeholder(placeholder)
        .render(theme, cx)
}

fn counted_area(
    cx: &Context<'_, FlowyApp>,
    theme: &flowy_ui::ResolvedTheme,
    id: &'static str,
    state: Entity<EditableTextState>,
    placeholder: String,
    count: String,
    height: f32,
) -> impl IntoElement {
    let input = theme::TextField::new(id, placeholder.clone(), state)
        .placeholder(placeholder)
        .multiline(true)
        .render(theme, cx)
        .min_h(px(height));
    div().w_full().flex().flex_col().child(input).child(
        div()
            .text_size(theme.caption_size())
            .text_color(theme.muted())
            .child(count),
    )
}

fn labeled_area(
    app: &FlowyApp,
    cx: &Context<'_, FlowyApp>,
    id: &'static str,
    state: Entity<EditableTextState>,
    placeholder: String,
    count: String,
    height: f32,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .child(field_label(app.t(if id == "more-about" {
            "more.about"
        } else {
            "more.instructions"
        })))
        .child(counted_area(
            cx,
            &app.theme,
            id,
            state,
            placeholder,
            count,
            height,
        ))
}

fn field_label(text: String) -> impl IntoElement {
    div().pb_1().text_sm().child(text)
}

fn step_heading(
    theme: &flowy_ui::ResolvedTheme,
    number: &str,
    title: String,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .size(px(22.))
                .rounded_full()
                .bg(theme.soft())
                .flex()
                .items_center()
                .justify_center()
                .text_size(theme.caption_size())
                .child(number.to_string()),
        )
        .child(
            div()
                .text_size(theme.body_size())
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(title),
        )
}

fn prompt_box(theme: &flowy_ui::ResolvedTheme, text: &str) -> impl IntoElement {
    let mut box_ = div()
        .w_full()
        .p_3()
        .rounded_lg()
        .bg(theme.soft())
        .flex()
        .flex_col()
        .gap_1();
    for line in text.lines() {
        let mut row = div()
            .text_size(theme.caption_size())
            .text_color(theme.muted());
        row = if line.is_empty() {
            row.h(px(8.))
        } else {
            row.child(line.to_string())
        };
        box_ = box_.child(row);
    }
    box_
}

fn card_mark(theme: &flowy_ui::ResolvedTheme) -> impl IntoElement {
    div()
        .size(px(28.))
        .rounded_md()
        .bg(theme.soft())
        .flex()
        .items_center()
        .justify_center()
        .child(theme::icons::file_plus())
}

fn menu_icon(page: Page) -> AnyElement {
    match page {
        Page::Computer => theme::icons::monitor().into_any_element(),
        Page::Skills => theme::icons::sparkles().into_any_element(),
        Page::Personal => theme::icons::sliders_horizontal().into_any_element(),
    }
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
