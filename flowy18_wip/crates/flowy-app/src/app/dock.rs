use super::{FlowyApp, Section};
use flowy_ui as theme;
use gpui::{Context, Window, div, prelude::*, px};

#[derive(Clone, Copy)]
enum DockIcon {
    NewTask,
    // Search,
    Library,
    Create,
    Automation,
    Plugins,
    More,
    Task,
}

impl DockIcon {
    fn render(self) -> theme::icons::Icon {
        match self {
            Self::NewTask => theme::icons::square_pen(),
            // Self::Search => theme::icons::search(),
            Self::Library => theme::icons::library(),
            Self::Create => theme::icons::plus(),
            Self::Automation => theme::icons::workflow(),
            Self::Plugins => theme::icons::puzzle(),
            Self::More => theme::icons::ellipsis(),
            Self::Task => theme::icons::list_check(),
        }
    }
}

const USER_MENU_WIDTH: f32 = 288.0;

const NAV: &[(&str, &str, Section, DockIcon)] = &[
    // ("search", "dock.search", Section::Search, DockIcon::Search),
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
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let name = self.auth.session.as_ref().map_or_else(
            || self.t("dock.user_fallback"),
            |session| session.display_name.clone(),
        );
        let initial = name.chars().next().unwrap_or('F').to_string();

        div()
            .id("dock")
            .w(px(theme::DOCK_WIDTH))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(self.theme.dock())
            .border_r_1()
            .border_color(self.theme.line())
            .child(self.dock_scroll(&name, window, cx))
            .child(self.dock_footer(&name, &initial, cx))
    }

    fn dock_scroll(
        &self,
        _name: &str,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let mut navigation =
            div().flex().flex_col().gap_1().child(Self::nav_item(
                &self.theme,
                "new-task",
                self.t("dock.new_task"),
                self.section == Section::Studio,
                Section::Studio,
                DockIcon::NewTask,
                cx,
            ));
        // Independent Agent App entry — deferred.
        // .child(Self::nav_item(&self.theme,
        //     "agent",
        //     self.tf("dock.agent", &[("name", name)]),
        //     false,
        //     cx,
        // ));

        for (id, key, section, icon) in NAV {
            navigation = if *id == "more" {
                navigation.child(self.more_dock_item(*icon, cx))
            } else {
                let selected = self.section == *section;
                navigation.child(Self::nav_item(
                    &self.theme,
                    id,
                    self.t(key),
                    selected,
                    *section,
                    *icon,
                    cx,
                ))
            };
        }

        div()
            .id("dock-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .pt_4()
            .pb_3()
            .child(brand())
            .child(navigation)
            .child(self.project_group_label(cx))
            .child(self.project_rows(window, cx))
            .child(group_label(&self.theme, &self.t("dock.tasks")))
            .child(self.task_rows(cx))
    }

    fn nav_item(
        theme: &flowy_ui::ResolvedTheme,
        id: &'static str,
        label: String,
        selected: bool,
        section: Section,
        icon: DockIcon,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        item_row(*theme, id, label, selected, icon).on_click(cx.listener(
            move |app, _event, _window, cx| {
                app.select_navigation(id, section, cx);
            },
        ))
    }

    pub(super) fn select_navigation(
        &mut self,
        id: &str,
        section: Section,
        cx: &mut Context<'_, Self>,
    ) {
        self.persona.menu = false;
        self.user_menu = false;
        if id == "new-task" {
            self.studio.update(cx, flowy_studio::StudioPanel::new_task);
            self.push_section(Section::Studio, cx);
            return;
        }
        self.push_section(section, cx);
    }

    fn more_dock_item(
        &self,
        icon: DockIcon,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let selected = self.section == Section::More;
        theme::anchor_trigger(
            item_row(self.theme, "more", self.t("dock.more"), selected, icon),
            "more-trigger",
            &self.ui,
        )
        .on_click(cx.listener(|app, _event, _window, cx| {
            app.user_menu = false;
            app.toggle_more(cx);
        }))
    }

    fn project_rows(
        &self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let mut rows = div().flex().flex_col();
        let (mut projects, sessions, active_project_id) = {
            let studio = self.studio.read(cx);
            (
                studio.projects.clone(),
                studio.agent.list_sessions(),
                studio.active_project_id.clone(),
            )
        };
        projects.sort_by_key(|project| !project.pinned);
        let active_session = self.studio.read(cx).agent.active_session_id();
        for project in projects {
            let project_id = project.id.clone();
            let expanded = self.expanded_projects.contains(&project_id);
            let selected =
                active_project_id.as_deref() == Some(project_id.as_str());
            let toggle_icon = if expanded {
                theme::icons::chevron_down()
            } else {
                theme::icons::folder()
            };
            let row = div()
                .id(format!("project-row-{project_id}"))
                .mx_2()
                .my_1()
                .pl_1()
                .pr_1()
                .h(px(36.))
                .rounded_lg()
                .flex()
                .items_center()
                .gap_1()
                .bg(if selected || expanded {
                    self.theme.dock_selected()
                } else {
                    self.theme.dock()
                })
                .when(!selected && !expanded, |row| {
                    row.hover(|row| row.bg(self.theme.dock_hover()))
                })
                .child(
                    theme::IconButton::new(
                        format!("project-expand-{project_id}"),
                        if expanded {
                            self.tf(
                                "project.collapse",
                                &[("name", &project.name)],
                            )
                        } else {
                            self.tf(
                                "project.expand",
                                &[("name", &project.name)],
                            )
                        },
                        toggle_icon,
                    )
                    .size(theme::ControlSize::Small)
                    .render(
                        &self.theme,
                        cx.listener({
                            let id = project_id.clone();
                            move |app, _event, _window, cx| {
                                app.toggle_project_expanded(&id, cx);
                            }
                        }),
                    ),
                )
                .child(
                    theme::interactive(
                        format!("project-open-{project_id}"),
                        project.name.clone(),
                        &self.theme,
                    )
                    .h_full()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .items_center()
                    .whitespace_nowrap()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(self.theme.body_size())
                            .child(project.name.clone()),
                    )
                    .when(project.pinned, |row| {
                        row.child(
                            theme::icons::pin().text_color(self.theme.muted()),
                        )
                    })
                    .on_click(cx.listener({
                        let id = project_id.clone();
                        move |app, _event, _window, cx| {
                            app.user_menu = false;
                            app.open_project(&id, cx);
                        }
                    })),
                );
            let row = if expanded {
                let menu_anchor = format!("project-menu-trigger-{project_id}");
                row.child(theme::anchor_trigger(
                    theme::IconButton::new(
                        format!("project-menu-button-{project_id}"),
                        self.t("project.menu"),
                        theme::icons::ellipsis(),
                    )
                    .size(theme::ControlSize::Small)
                    .render(
                        &self.theme,
                        cx.listener({
                            let id = project_id.clone();
                            move |app, _event, _window, cx| {
                                app.toggle_project_menu(&id, cx);
                            }
                        }),
                    ),
                    menu_anchor,
                    &self.ui,
                ))
                .child(theme::tooltip_trigger(
                    format!("project-new-task-tip-{project_id}"),
                    theme::IconButton::new(
                        format!("project-new-task-{project_id}"),
                        self.t("project.new_task"),
                        theme::icons::square_pen(),
                    )
                    .size(theme::ControlSize::Small)
                    .render(
                        &self.theme,
                        cx.listener({
                            let id = project_id.clone();
                            move |app, _event, _window, cx| {
                                app.project_new_task(&id, cx);
                            }
                        }),
                    ),
                    self.t("project.new_task"),
                    &self.ui,
                    window,
                    cx,
                    self.i18n.locale().is_rtl(),
                ))
            } else {
                row
            };
            rows = rows.child(row);

            if expanded {
                for session in sessions.iter().filter(|session| {
                    session.project_id.as_deref() == Some(project_id.as_str())
                }) {
                    rows = rows.child(self.project_task_row(
                        &session.id,
                        if session.title.is_empty() {
                            self.t("dock.new_task")
                        } else {
                            session.title.clone()
                        },
                        active_session.as_deref() == Some(session.id.as_str()),
                        cx,
                    ));
                }
            }
        }
        rows
    }

    fn project_task_row(
        &self,
        session_id: &str,
        title: String,
        selected: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let id = format!("project-task-{session_id}");
        let session_id = session_id.to_owned();
        theme::interactive(id, title.clone(), &self.theme)
            .mx_2()
            .ml(px(18.))
            .h(px(34.))
            .px_2()
            .rounded_md()
            .flex()
            .items_center()
            .gap_2()
            .when(selected, |row| row.bg(self.theme.dock_selected()))
            .when(!selected, |row| {
                row.hover(|row| row.bg(self.theme.dock_hover()))
            })
            .child(theme::icons::book().text_color(self.theme.muted()))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(self.theme.body_size())
                    .child(title),
            )
            .on_click(cx.listener(move |app, _event, _window, cx| {
                app.user_menu = false;
                app.section = Section::Studio;
                app.studio.update(cx, |panel, cx| {
                    panel.open_session(&session_id, cx);
                });
                cx.notify();
            }))
    }

    fn task_rows(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let studio = self.studio.read(cx);
        let sessions: Vec<_> = studio
            .agent
            .list_sessions()
            .into_iter()
            .filter(|session| {
                session.project_id.as_ref().is_none_or(|project_id| {
                    !studio
                        .projects
                        .iter()
                        .any(|project| &project.id == project_id)
                })
            })
            .collect();
        let active = studio.agent.active_session_id();
        if sessions.is_empty() {
            return div()
                .px_4()
                .py_1()
                .text_size(self.theme.body_size())
                .text_color(self.theme.muted())
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
                item_row(self.theme, id, title, selected, DockIcon::Task)
                    .on_click(cx.listener(move |app, _event, _window, cx| {
                        app.user_menu = false;
                        app.section = Section::Studio;
                        app.studio.update(cx, |panel, cx| {
                            panel.open_session(&session_id, cx);
                        });
                        cx.notify();
                    })),
            );
        }
        rows
    }

    fn project_group_label(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .px_4()
            .pt_4()
            .pb_1()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(self.theme.caption_size())
                    .text_color(self.theme.muted())
                    .child(self.t("dock.projects")),
            )
            .child(
                theme::IconButton::new(
                    "new-project",
                    self.t("dock.new_project"),
                    theme::icons::plus(),
                )
                .size(theme::ControlSize::Small)
                .render(
                    &self.theme,
                    cx.listener(|app, _event, _window, cx| {
                        app.user_menu = false;
                        app.open_project_create(cx);
                    }),
                ),
            )
    }

    fn dock_footer(
        &self,
        name: &str,
        initial: &str,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .border_t_1()
            .border_color(self.theme.line())
            .px_3()
            .py_2()
            .child(
                theme::anchor_trigger(
                    theme::interactive(
                        "dock-user",
                        name.to_string(),
                        &self.theme,
                    )
                    .px_1()
                    .rounded_lg()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .hover(|row| row.bg(self.theme.soft()))
                    .on_click(cx.listener(
                        |app, _event, _window, cx| {
                            cx.stop_propagation();
                            app.toggle_user_menu(cx);
                        },
                    )),
                    "dock-user",
                    &self.ui,
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .min_w_0()
                        .child(avatar(&self.theme, initial))
                        .child(
                            div()
                                .h(px(28.))
                                .flex()
                                .items_center()
                                .text_size(self.theme.body_size())
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .child(name.to_string()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            theme::icons::monitor()
                                .text_color(self.theme.muted()),
                        )
                        .child(
                            theme::icons::bell().text_color(self.theme.muted()),
                        ),
                ),
            )
    }

    pub(super) fn toggle_user_menu(&mut self, cx: &mut Context<'_, Self>) {
        self.studio.update(cx, |panel, cx| {
            panel.model_menu_open = false;
            cx.notify();
        });
        self.persona.menu = false;
        self.user_menu = !self.user_menu;
        cx.notify();
    }

    pub(super) fn close_user_menu(&mut self, cx: &mut Context<'_, Self>) {
        if !self.user_menu {
            return;
        }
        self.user_menu = false;
        cx.notify();
    }

    pub fn user_menu_content(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let name = self.auth.session.as_ref().map_or_else(
            || self.t("dock.user_fallback"),
            |session| session.display_name.clone(),
        );
        let initial = name.chars().next().unwrap_or('F').to_string();
        let credits = self.studio.read(cx).credits_label();
        self.user_menu_card(name, initial, credits, cx)
    }

    fn user_menu_card(
        &self,
        name: String,
        initial: String,
        credits: String,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        theme::menu(
            "user-menu",
            self.t("dock.user_fallback"),
            &self.theme,
            &self.ui,
            cx,
        )
        .w(px(USER_MENU_WIDTH))
        .p_1()
        .gap_1()
        .on_click(cx.listener(|_app, _event, _window, cx| {
            cx.stop_propagation();
        }))
        .child(self.user_menu_header(&name, &initial))
        .child(self.user_menu_credits(&credits, cx))
        .child(self.user_menu_row(
            "user-account",
            theme::icons::user(),
            self.t("settings.account"),
            false,
            cx,
            |app, cx| {
                app.open_settings_page(super::settings::Page::Account, cx);
            },
        ))
        .child(self.user_menu_row(
            "user-settings",
            theme::icons::sliders_horizontal(),
            self.t("dock.settings"),
            false,
            cx,
            |app, cx| {
                app.open_settings_page(super::settings::Page::General, cx);
            },
        ))
        .child(menu_divider(&self.theme))
        .child(self.user_menu_row(
            "user-home",
            theme::icons::house(),
            self.t("dock.home"),
            true,
            cx,
            |app, cx| {
                app.user_menu = false;
                app.note("notice.cloud_later", cx);
            },
        ))
        .child(self.user_menu_row(
            "user-help",
            theme::icons::circle_help(),
            self.t("settings.help"),
            true,
            cx,
            |app, cx| {
                app.open_settings_page(super::settings::Page::Help, cx);
            },
        ))
        .child(self.user_menu_row(
            "user-docs",
            theme::icons::book(),
            self.t("dock.docs"),
            true,
            cx,
            |app, cx| {
                app.user_menu = false;
                app.note("notice.cloud_later", cx);
            },
        ))
        .child(menu_divider(&self.theme))
        .child(self.user_menu_sign_out(cx))
    }

    fn user_menu_header(&self, name: &str, initial: &str) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .flex()
            .items_center()
            .gap_2()
            .child(avatar(&self.theme, initial))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(self.theme.body_size())
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(name.to_string()),
                    )
                    .child(
                        div()
                            .text_size(self.theme.caption_size())
                            .text_color(self.theme.muted())
                            .child(self.t("settings.personal_plan")),
                    ),
            )
            .child(
                theme::icons::chevrons_up_down().text_color(self.theme.muted()),
            )
    }

    fn user_menu_credits(
        &self,
        credits: &str,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .mx_1()
            .mb_0()
            .px_3()
            .py_2()
            .rounded_lg()
            .bg(self.theme.account_menu_surface())
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(self.theme.body_size())
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(
                                self.t(self.studio.read(cx).plan.label_key()),
                            ),
                    )
                    .child(
                        theme::Button::new(
                            "user-upgrade",
                            self.t("studio.upgrade"),
                        )
                        .variant(theme::ControlVariant::Primary)
                        .render(
                            &self.theme,
                            cx.listener(|app, _event, _window, cx| {
                                cx.stop_propagation();
                                app.user_menu = false;
                                app.note("notice.upgrade_later", cx);
                            }),
                        ),
                    ),
            )
            .child(
                theme::interactive(
                    "user-credits",
                    self.t("credits.balance"),
                    &self.theme,
                )
                .flex()
                .items_center()
                .justify_between()
                .hover(|row| row.bg(self.theme.account_menu_hover()))
                .on_click(cx.listener(|app, _event, _window, cx| {
                    cx.stop_propagation();
                    app.user_menu = false;
                    app.open_invite(cx);
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(theme::icons::sparkles())
                        .child(
                            div()
                                .text_size(self.theme.body_size())
                                .child(self.t("credits.balance")),
                        )
                        .child(
                            theme::icons::info()
                                .size(px(14.))
                                .text_color(self.theme.muted()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .text_size(self.theme.body_size())
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .child(credits.to_owned()),
                        )
                        .child(
                            theme::icons::chevron_right()
                                .text_color(self.theme.muted()),
                        ),
                ),
            )
    }

    fn user_menu_row(
        &self,
        id: &'static str,
        icon: impl IntoElement,
        label: String,
        external: bool,
        cx: &Context<'_, Self>,
        action: impl Fn(&mut Self, &mut Context<'_, Self>) + 'static,
    ) -> impl IntoElement {
        theme::interactive(id, label.clone(), &self.theme)
            .px_2()
            .py(px(1.))
            .rounded_md()
            .flex()
            .items_center()
            .gap_2()
            .hover(|row| row.bg(self.theme.account_menu_hover()))
            .on_click(cx.listener(move |app, _event, _window, cx| {
                cx.stop_propagation();
                action(app, cx);
            }))
            .child(icon)
            .child(
                div()
                    .flex_1()
                    .text_size(self.theme.body_size())
                    .child(label),
            )
            .when(external, |row| {
                row.child(
                    theme::icons::arrow_up_right()
                        .text_color(self.theme.muted()),
                )
            })
    }

    fn user_menu_sign_out(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let label = self.t("dock.sign_out");
        theme::interactive("user-sign-out", label.clone(), &self.theme)
            .px_2()
            .py(px(1.))
            .rounded_md()
            .flex()
            .items_center()
            .gap_2()
            .text_color(self.theme.danger())
            .hover(|row| row.bg(self.theme.account_menu_hover()))
            .on_click(cx.listener(|app, _event, _window, cx| {
                cx.stop_propagation();
                app.user_menu = false;
                app.sign_out(cx);
            }))
            .child(theme::icons::log_out())
            .child(div().text_size(self.theme.body_size()).child(label))
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

fn group_label(
    theme: &flowy_ui::ResolvedTheme,
    text: &str,
) -> impl IntoElement {
    div()
        .px_4()
        .pt_4()
        .pb_1()
        .text_size(theme.caption_size())
        .text_color(theme.muted())
        .child(text.to_string())
}

fn item_row(
    theme: flowy_ui::ResolvedTheme,
    id: impl Into<gpui::ElementId>,
    label: String,
    selected: bool,
    icon: DockIcon,
) -> gpui::Stateful<gpui::Div> {
    theme::nav_item(id, label, selected, icon.render(), &theme)
}

fn avatar(theme: &flowy_ui::ResolvedTheme, initial: &str) -> impl IntoElement {
    div()
        .size(px(28.))
        .rounded_full()
        .bg(theme.ink())
        .text_color(theme.on_ink())
        .flex()
        .items_center()
        .justify_center()
        .text_size(theme.caption_size())
        .child(initial.to_string())
}

fn menu_divider(theme: &flowy_ui::ResolvedTheme) -> impl IntoElement {
    div().my_1().mx_1().h(px(1.)).bg(theme.line())
}
