use super::{FlowyApp, Section};
use flowy_ui as theme;
use gpui::{Context, Window, div, prelude::*, px};

#[derive(Clone, Copy)]
enum DockIcon {
    NewTask,
    Search,
    Library,
    Create,
    Automation,
    Plugins,
    More,
    NewProject,
    Project,
    Task,
}

impl DockIcon {
    fn render(self) -> gpui::Svg {
        match self {
            Self::NewTask => theme::icons::square_pen(),
            Self::Search => theme::icons::search(),
            Self::Library => theme::icons::library(),
            Self::Create => theme::icons::plus(),
            Self::Automation => theme::icons::workflow(),
            Self::Plugins => theme::icons::puzzle(),
            Self::More => theme::icons::ellipsis(),
            Self::NewProject => theme::icons::folder_plus(),
            Self::Project => theme::icons::folder(),
            Self::Task => theme::icons::list_check(),
        }
    }
}

const NAV: &[(&str, &str, Section, DockIcon)] = &[
    ("search", "dock.search", Section::Search, DockIcon::Search),
    (
        "library",
        "dock.library",
        Section::Library,
        DockIcon::Library,
    ),
    ("create", "dock.create", Section::Create, DockIcon::Create),
    (
        "automation",
        "dock.automation",
        Section::Automation,
        DockIcon::Automation,
    ),
    (
        "plugins",
        "dock.plugins",
        Section::Plugins,
        DockIcon::Plugins,
    ),
    ("more", "dock.more", Section::More, DockIcon::More),
];

impl FlowyApp {
    pub fn dock(
        &self,
        _window: &mut Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let name = self.auth.session.as_ref().map_or_else(
            || self.t("dock.user_fallback"),
            |session| session.display_name.clone(),
        );
        let initial = name.chars().next().unwrap_or('F').to_string();
        let studio = self.studio.read(cx);
        let credits = grouped(studio.credits);

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
                Section::Studio,
                DockIcon::NewTask,
                cx,
            ));
        // Independent Agent App entry — deferred.
        // .child(Self::nav_item(
        //     "agent",
        //     self.tf("dock.agent", &[("name", name)]),
        //     false,
        //     cx,
        // ));

        for (id, key, section, icon) in NAV {
            column = if *id == "more" {
                column.child(self.more_dock_item(*icon, cx))
            } else {
                let selected = self.section == *section;
                column.child(Self::nav_item(
                    id,
                    self.t(key),
                    selected,
                    *section,
                    *icon,
                    cx,
                ))
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
        section: Section,
        icon: DockIcon,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        item_row(id, label, selected, icon).on_click(cx.listener(
            move |app, _event, _window, cx| {
                app.section = section;
                app.persona.menu = false;
                if id == "new-task" {
                    app.studio.update(cx, |panel, cx| panel.new_task(cx));
                    app.section = Section::Studio;
                } else if section == Section::Studio {
                    app.notice = None;
                }
                app.sync_studio_surface(cx);
                cx.notify();
            },
        ))
    }

    fn more_dock_item(
        &self,
        icon: DockIcon,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let selected = self.section == Section::More;
        item_row("more", self.t("dock.more"), selected, icon).on_click(
            cx.listener(|app, _event, _window, cx| {
                app.toggle_more(cx);
            }),
        )
    }

    fn project_rows(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut rows = div().flex().flex_col();
        rows = rows.child(
            item_row(
                "new-project",
                self.t("dock.new_project"),
                false,
                DockIcon::NewProject,
            )
            .on_click(cx.listener(|app, _event, _window, cx| {
                app.add_project(cx);
            })),
        );
        let projects = self.studio.read(cx).projects.clone();
        for (slot, index) in projects.iter().enumerate() {
            let id = format!("project-{slot}");
            let label =
                self.tf("project.untitled", &[("index", &index.to_string())]);
            rows = rows.child(item_row(id, label, false, DockIcon::Project));
        }
        rows
    }

    fn task_rows(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let studio = self.studio.read(cx);
        let sessions = studio.agent.list_sessions();
        let active = studio.agent.active_session_id();
        if sessions.is_empty() {
            return div()
                .px_4()
                .py_1()
                .text_sm()
                .text_color(theme::muted())
                .child(self.t("dock.no_tasks"));
        }
        let mut rows = div().flex().flex_col();
        for (index, session) in sessions.into_iter().enumerate() {
            let id = format!("task-{index}");
            let session_id = session.id.clone();
            let title = if session.title.is_empty() {
                self.t("dock.new_task")
            } else {
                session.title
            };
            let selected = active.as_deref() == Some(session_id.as_str());
            rows = rows.child(
                item_row(id, title, selected, DockIcon::Task).on_click(
                    cx.listener(move |app, _event, _window, cx| {
                        app.section = Section::Studio;
                        app.studio.update(cx, |panel, cx| {
                            panel.open_session(&session_id, cx);
                        });
                        cx.notify();
                    }),
                ),
            );
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
    icon: DockIcon,
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
        .child(icon.render())
        .child(
            div()
                .text_sm()
                .overflow_hidden()
                .whitespace_nowrap()
                .child(label),
        )
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
