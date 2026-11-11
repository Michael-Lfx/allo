use super::{FlowyApp, Section};
use crate::theme;
use gpui::{Context, Window, div, prelude::*, px};

const NAV: &[(&str, &str, Section)] = &[
    // ("computer", "dock.computer", Section::Computer),
    ("search", "dock.search", Section::Search),
    ("library", "dock.library", Section::Library),
    ("create", "dock.create", Section::Create),
    ("automation", "dock.automation", Section::Automation),
    ("plugins", "dock.plugins", Section::Plugins),
    ("more", "dock.more", Section::More),
];

impl FlowyApp {
    pub(super) fn dock(
        &self,
        _window: &mut Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let name = self.auth.session.as_ref().map_or_else(
            || self.t("dock.user_fallback"),
            |session| session.display_name.clone(),
        );
        let initial = name.chars().next().unwrap_or('F').to_string();
        let credits = grouped(self.studio.credits);

        div()
            .id("dock")
            .w(px(theme::DOCK_WIDTH))
            .h_full()
            .flex()
            .flex_col()
            .bg(theme::dock())
            .border_r_1()
            .border_color(theme::line())
            .child(self.dock_scroll(&name, cx))
            .child(self.dock_footer(&name, &initial, &credits, cx))
    }

    fn dock_scroll(
        &self,
        name: &str,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let mut column = div()
            .id("dock-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .pt_4()
            .pb_3()
            .child(brand())
            .child(Self::nav_item(
                "new-task",
                self.t("dock.new_task"),
                self.section == Section::Studio,
                cx,
            ))
            .child(Self::nav_item(
                "agent",
                self.tf("dock.agent", &[("name", name)]),
                self.section == Section::Agent,
                cx,
            ));

        for (id, key, section) in NAV {
            column = if *id == "more" {
                column.child(self.more_dock_item(cx))
            } else {
                let selected = self.section == *section;
                column.child(Self::nav_item(id, self.t(key), selected, cx))
            };
        }

        column
            .child(group_label(&self.t("dock.projects")))
            .child(self.project_rows(cx))
            .child(group_label(&self.t("dock.tasks")))
            .child(self.task_rows(cx))
    }

    fn nav_item(
        id: &'static str,
        label: String,
        selected: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let section = section_for(id);
        item_row(id, label, selected).on_click(cx.listener(
            move |app, _event, _window, cx| {
                app.section = section;
                app.persona.menu = false;
                if id == "new-task" {
                    app.clear_thread(cx);
                } else if section == Section::Studio {
                    app.notice = None;
                    cx.notify();
                } else {
                    cx.notify();
                }
            },
        ))
    }

    fn more_dock_item(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let selected = self.section == Section::More;
        item_row("more", self.t("dock.more"), selected).on_click(cx.listener(
            |app, _event, _window, cx| {
                app.toggle_more(cx);
            },
        ))
    }

    fn project_rows(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut rows = div().flex().flex_col();
        rows = rows.child(
            item_row("new-project", self.t("dock.new_project"), false)
                .on_click(
                    cx.listener(|app, _event, _window, cx| app.add_project(cx)),
                ),
        );
        for (slot, index) in self.studio.projects.iter().enumerate() {
            let id = format!("project-{slot}");
            let label =
                self.tf("project.untitled", &[("index", &index.to_string())]);
            rows = rows.child(item_row(id, label, false));
        }
        rows
    }

    fn task_rows(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        if self.studio.tasks.is_empty() {
            return div()
                .px_4()
                .py_1()
                .text_sm()
                .text_color(theme::muted())
                .child(self.t("dock.no_tasks"));
        }
        let active = self
            .studio
            .thread
            .as_ref()
            .map(|thread| thread.title.as_str());
        let mut rows = div().flex().flex_col();
        for (index, task) in self.studio.tasks.iter().enumerate() {
            let id = format!("task-{index}");
            let title = task.clone();
            let selected = active == Some(title.as_str());
            rows = rows.child(item_row(id, title.clone(), selected).on_click(
                cx.listener(move |app, _event, _window, cx| {
                    app.open_thread(title.clone(), cx);
                }),
            ));
        }
        rows
    }

    fn dock_footer(
        &self,
        name: &str,
        initial: &str,
        credits: &str,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .border_t_1()
            .border_color(theme::line())
            .px_3()
            .py_3()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .id("credits")
                    .px_2()
                    .py_2()
                    .rounded_lg()
                    .bg(theme::soft())
                    .flex()
                    .flex_col()
                    .gap_1()
                    .cursor_pointer()
                    .on_click(cx.listener(|app, _event, _window, cx| {
                        app.open_invite(cx);
                    }))
                    .child(
                        div().text_sm().child(
                            self.tf("dock.credits", &[("count", credits)]),
                        ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::muted())
                            .child(self.t("dock.invite")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_1()
                    .child(
                        div()
                            .id("open-settings")
                            .flex()
                            .items_center()
                            .gap_2()
                            .cursor_pointer()
                            .child(avatar(initial))
                            .child(div().text_sm().child(name.to_string()))
                            .on_click(cx.listener(
                                |app, _event, _window, cx| {
                                    app.open_settings(cx);
                                },
                            )),
                    )
                    .child(
                        div()
                            .id("sign-out")
                            .text_xs()
                            .text_color(theme::muted())
                            .cursor_pointer()
                            .child(self.t("dock.sign_out"))
                            .on_click(cx.listener(
                                |app, _event, _window, cx| {
                                    app.sign_out(cx);
                                },
                            )),
                    ),
            )
    }
}

fn section_for(id: &str) -> Section {
    match id {
        "agent" => Section::Agent,
        "search" => Section::Search,
        "library" => Section::Library,
        "create" => Section::Create,
        "automation" => Section::Automation,
        "plugins" => Section::Plugins,
        "more" => Section::More,
        _ => Section::Studio,
    }
}

fn brand() -> impl IntoElement {
    div()
        .px_4()
        .pb_3()
        .flex()
        .items_center()
        .gap_2()
        .child(super::product_mark(22.))
        .child(
            div()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(crate::brand::product().to_string()),
        )
}

fn group_label(text: &str) -> impl IntoElement {
    div()
        .px_4()
        .pt_4()
        .pb_1()
        .text_xs()
        .text_color(theme::muted())
        .child(text.to_string())
}

fn item_row(
    id: impl Into<gpui::ElementId>,
    label: String,
    selected: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .mx_2()
        .px(theme::space(8.))
        .py(theme::space(4.))
        .rounded_md()
        .flex()
        .items_center()
        .gap_2()
        .bg(if selected {
            theme::card()
        } else {
            theme::dock()
        })
        .when(selected, gpui::Styled::shadow_sm)
        .cursor_pointer()
        .hover(|row| row.bg(theme::card()))
        .child(icon_mark())
        .child(
            div()
                .text_sm()
                .overflow_hidden()
                .whitespace_nowrap()
                .child(label),
        )
}

fn icon_mark() -> impl IntoElement {
    div()
        .size(px(16.))
        .rounded(px(4.))
        .border_1()
        .border_color(theme::line())
        .flex()
        .items_center()
        .justify_center()
        .child(div().size(px(6.)).rounded_full().bg(theme::ink()))
}

fn avatar(initial: &str) -> impl IntoElement {
    div()
        .size(px(28.))
        .rounded_full()
        .bg(theme::ink())
        .text_color(theme::on_ink())
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .child(initial.to_string())
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
