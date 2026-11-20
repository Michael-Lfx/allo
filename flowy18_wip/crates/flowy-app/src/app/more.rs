use super::{FlowyApp, Section};
use crate::persona::{
    ABOUT_MAX, INSTRUCTIONS_MAX, KNOWLEDGE_MAX, Knowledge, NAME_MAX, Profile,
    limited, read_patch, save,
};
use flowy_ui as theme;
use gpui::{
    ClipboardItem, Context, Entity, Pixels, Size, Window, div, prelude::*, px,
};
use gpui_ce_elements::editable_text::EditableTextState;

mod personalization;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Page {
    Skills,
    Personal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tab {
    Profile,
    Knowledge,
}

const PASTE_MAX: usize = 8000;

impl FlowyApp {
    pub fn more_canvas(
        &self,
        window: &mut gpui::Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let mut page = div()
            .id("more-page")
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col();
        page = if self.persona.page == Page::Skills {
            page.child(self.skills_canvas(window, cx))
        } else {
            page.child(self.personal_canvas(window, cx))
        };
        page
    }

    pub(super) fn select_more_page(
        &mut self,
        page: Page,
        cx: &mut Context<'_, Self>,
    ) {
        self.persona.page = page;
        self.notice = None;
        if self.section == Section::More {
            self.sync_studio_surface(cx);
            cx.notify();
        } else {
            self.push_section(Section::More, cx);
        }
    }

    pub(super) fn more_layer(
        &self,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let frame = self.theme.modal_size(window.viewport_size());
        let content =
            if self.active_dialog == super::state::ActiveDialog::Knowledge {
                self.knowledge_dialog(frame, cx).into_any_element()
            } else {
                self.import_dialog(frame, cx).into_any_element()
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

    fn import_dialog(
        &self,
        frame: Size<Pixels>,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let prompt = self.import_prompt();
        let ready = !self
            .persona
            .import_paste
            .read(cx)
            .as_str()
            .trim()
            .is_empty();
        let mut body = dialog_fields()
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
            body = body.child(
                div()
                    .text_size(self.theme.caption_size())
                    .text_color(self.theme.muted())
                    .child(self.t(key)),
            );
        }
        let paste_label = self.t("more.import_step_paste");
        let body = body
            .child(step_heading(&self.theme, "2", paste_label.clone()))
            .child(
                div()
                    .text_size(self.theme.caption_size())
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(self.t("more.import_step_paste_body")),
            )
            .child(
                theme::TextField::new(
                    "import-paste",
                    paste_label,
                    self.persona.import_paste.clone(),
                )
                .placeholder(self.t("more.import_placeholder"))
                .quiet()
                .multiline_height(px(120.))
                .render(&self.theme, cx),
            );
        dialog_shell(
            self,
            "import-memory-dialog",
            "more.import_title",
            frame,
            cx,
        )
        .child(self.dialog_title("more.import_title", cx))
        .child(dialog_body("import-memory-scroll", body))
        .child(self.dialog_actions(
            "cancel-import",
            "confirm-import",
            "more.import_action",
            ready,
            cx,
        ))
    }

    fn knowledge_dialog(
        &self,
        frame: Size<Pixels>,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
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
        let body = dialog_fields()
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
            ));
        dialog_shell(self, "add-knowledge-dialog", "more.add_title", frame, cx)
            .child(self.dialog_title("more.add_title", cx))
            .child(dialog_body("add-knowledge-scroll", body))
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
            .flex_shrink_0()
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
            .flex_shrink_0()
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
    title: &'static str,
    frame: Size<Pixels>,
    cx: &Context<'_, FlowyApp>,
) -> gpui::Stateful<gpui::Div> {
    let theme = &app.theme;
    theme::dialog(id, app.t(title), theme, &app.ui, cx)
        .w(frame.width.min(px(480.)))
        .max_h(frame.height.min(px(640.)))
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

fn dialog_body(id: &'static str, fields: impl IntoElement) -> impl IntoElement {
    div()
        .id(id)
        .w_full()
        .min_h_0()
        .flex_auto()
        .overflow_y_scroll()
        .child(fields)
}

fn dialog_fields() -> gpui::Div {
    div().w_full().min_w_0().flex().flex_col().gap_3()
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
        .multiline_height(px(height))
        .render(theme, cx);
    div().w_full().flex().flex_col().child(input).child(
        div()
            .text_size(theme.caption_size())
            .text_color(theme.muted())
            .child(count),
    )
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
