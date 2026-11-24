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
    Skills,
    Personalization,
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
            Self::Skills => theme::icons::sparkles(),
            Self::Personalization => theme::icons::sliders_horizontal(),
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
            let selected = self.section == *section;
            navigation = navigation.child(Self::nav_item(
                &self.theme,
                id,
                self.t(key),
                selected,
                *section,
                *icon,
                cx,
            ));
        }
        navigation = navigation
            .child(self.more_page_dock_item(
                "skills",
                "more.skills",
                super::more::Page::Skills,
                DockIcon::Skills,
                cx,
            ))
            .child(self.more_page_dock_item(
                "personalization",
                "more.personal",
                super::more::Page::Personal,
                DockIcon::Personalization,
                cx,
            ));

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
            .child(self.project_section(window, cx))
            .child(self.task_section(window, cx))
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
        let row = item_row(*theme, id, label, selected, icon);
        let row = if id == "new-task" {
            row.debug_selector(|| "dock-nav-item".into())
        } else {
            row
        };
        row.on_click(cx.listener(move |app, _event, _window, cx| {
            app.select_navigation(id, section, cx);
        }))
    }

    pub(super) fn select_navigation(
        &mut self,
        id: &str,
        section: Section,
        cx: &mut Context<'_, Self>,
    ) {
        self.user_menu = false;
        if id == "new-task" {
            self.start_library_conversation(cx);
            return;
        }
        self.push_section(section, cx);
    }

    fn more_page_dock_item(
        &self,
        id: &'static str,
        key: &'static str,
        page: super::more::Page,
        icon: DockIcon,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let selected =
            self.section == Section::More && self.persona.page == page;
        item_row(self.theme, id, self.t(key), selected, icon).on_click(
            cx.listener(move |app, _event, _window, cx| {
                app.user_menu = false;
                app.select_more_page(page, cx);
            }),
        )
    }

    fn project_rows(
        &self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let (mut projects, sessions) = {
            let studio = self.studio.read(cx);
            (studio.projects.clone(), studio.agent.list_sessions())
        };
        projects.sort_by_key(|project| !project.pinned);
        let active_session = self.studio.read(cx).agent.active_session_id();
        let mut rows = div().mt(px(2.)).flex().flex_col().gap(px(2.));
        for project in projects {
            let project_id = project.id.clone();
            let expanded = self.expanded_projects.contains(&project_id);
            let menu_open =
                self.project_menu.as_deref() == Some(project_id.as_str());
            let toggle_label = if expanded {
                self.tf("project.collapse", &[("name", &project.name)])
            } else {
                self.tf("project.expand", &[("name", &project.name)])
            };
            let hovered = window.use_keyed_state(
                gpui::SharedString::from(format!(
                    "project-row-hovered-{project_id}"
                )),
                cx,
                |_, _| false,
            );
            let row_hovered = *hovered.read(cx);
            let show_actions = row_hovered || menu_open;
            // The folder glyph stays put. The disclosure chevron replaces it
            // only while the pointer is on this row.
            let toggle_icon = if row_hovered {
                theme::icons::chevron_down()
            } else {
                theme::icons::folder_open()
            };
            let row =
                dock_nav_row(&self.theme, format!("project-row-{project_id}"))
                    .bg(self.theme.dock())
                    .when(show_actions, |row| {
                        row.bg(self.theme.dock_selected())
                    })
                    .hover(|row| row.bg(self.theme.dock_selected()))
                    .on_hover({
                        let hovered = hovered;
                        move |active, window, cx| {
                            let changed = hovered.update(cx, |state, cx| {
                                if *state == *active {
                                    return false;
                                }
                                *state = *active;
                                cx.notify();
                                true
                            });
                            if changed {
                                window.refresh();
                            }
                        }
                    })
                    .debug_selector(|| "project-row".into())
                    .child(project_folder_toggle(
                        window,
                        &self.theme,
                        format!("project-expand-{project_id}"),
                        toggle_label,
                        toggle_icon,
                        cx.listener({
                            let id = project_id.clone();
                            move |app, _event, _window, cx| {
                                app.toggle_project_expanded(&id, cx);
                            }
                        }),
                    ))
                    .child(
                        theme::interactive(
                            format!("project-name-{project_id}"),
                            project.name.clone(),
                            &self.theme,
                        )
                        .border_0()
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
                                theme::icons::pin()
                                    .text_color(self.theme.muted()),
                            )
                        })
                        .on_click(cx.listener({
                            let id = project_id.clone();
                            move |app, _event, _window, cx| {
                                app.user_menu = false;
                                app.toggle_project_expanded(&id, cx);
                            }
                        })),
                    )
                    .when(show_actions, |row| {
                        row.child(self.project_row_actions(
                            window,
                            cx,
                            &project_id,
                        ))
                    });
            rows = rows.child(row);

            if expanded {
                for session in sessions.iter().filter(|session| {
                    session.project_id.as_deref() == Some(project_id.as_str())
                }) {
                    rows = rows.child(self.conversation_row(
                        window,
                        cx,
                        format!("project-task-{}", session.id),
                        session.id.clone(),
                        if session.title.is_empty() {
                            self.t("dock.new_task")
                        } else {
                            session.title.clone()
                        },
                        active_session.as_deref() == Some(session.id.as_str()),
                        div().w(px(16.)).h(px(16.)).flex_shrink_0(),
                        "project-task-row",
                    ));
                }
            }
        }
        rows
    }

    fn project_row_actions(
        &self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
        project_id: &str,
    ) -> impl IntoElement {
        let menu_anchor = format!("project-menu-trigger-{project_id}");
        div()
            .id(format!("project-actions-{project_id}"))
            .flex()
            .items_center()
            .debug_selector(|| "project-actions".into())
            .child(theme::anchor_trigger(
                dock_icon_button(
                    format!("project-menu-button-hit-{project_id}"),
                    theme::IconButton::new(
                        format!("project-menu-button-{project_id}"),
                        self.t("project.menu"),
                        theme::icons::ellipsis(),
                    ),
                    &self.theme,
                    cx.listener({
                        let id = project_id.to_owned();
                        move |app, _event, _window, cx| {
                            app.toggle_project_menu(&id, cx);
                        }
                    }),
                ),
                menu_anchor,
                &self.ui,
            ))
            .child(
                theme::tooltip_trigger(
                    format!("project-new-task-tip-{project_id}"),
                    dock_icon_button(
                        format!("project-new-task-hit-{project_id}"),
                        theme::IconButton::new(
                            format!("project-new-task-{project_id}"),
                            self.t("project.new_task"),
                            theme::icons::square_pen(),
                        ),
                        &self.theme,
                        cx.listener({
                            let id = project_id.to_owned();
                            move |app, _event, window, cx| {
                                app.project_new_task(&id, window, cx);
                            }
                        }),
                    ),
                    self.t("project.new_task"),
                    &self.ui,
                    window,
                    cx,
                    self.i18n.locale().is_rtl(),
                )
                .debug_selector(|| "project-edit".into()),
            )
    }

    /// Same chrome as a project row, so the more button sits inside the hover
    /// fill and lines up with the project's edit button.
    #[expect(
        clippy::too_many_arguments,
        reason = "one row reads better than a struct built at each call site"
    )]
    fn conversation_row(
        &self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
        row_id: String,
        session_id: String,
        title: String,
        selected: bool,
        leading: impl IntoElement,
        selector: &'static str,
    ) -> impl IntoElement {
        let menu_open = self.task_menu.as_deref() == Some(session_id.as_str());
        let hovered = window.use_keyed_state(
            gpui::SharedString::from(format!(
                "conversation-row-hovered-{session_id}"
            )),
            cx,
            |_, _| false,
        );
        let show_menu = *hovered.read(cx) || menu_open;
        let open_id = session_id.clone();
        theme::interactive(row_id, title.clone(), &self.theme)
            .mx_2()
            .px(self.theme.space(8.))
            .py(self.theme.space(4.))
            .rounded_md()
            .flex()
            .items_center()
            .gap_2()
            .bg(if selected {
                self.theme.dock_selected()
            } else {
                self.theme.dock()
            })
            .when(show_menu, |row| row.bg(self.theme.dock_selected()))
            .hover(|row| row.bg(self.theme.dock_selected()))
            .on_hover({
                let hovered = hovered;
                move |active, window, cx| {
                    let changed = hovered.update(cx, |state, cx| {
                        if *state == *active {
                            return false;
                        }
                        *state = *active;
                        cx.notify();
                        true
                    });
                    if changed {
                        window.refresh();
                    }
                }
            })
            .debug_selector(move || selector.into())
            .child(leading)
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(self.theme.body_size())
                    .child(title),
            )
            .when(show_menu, |row| {
                row.child(self.conversation_row_actions(
                    window,
                    cx,
                    &session_id,
                ))
            })
            .on_click(cx.listener(move |app, _event, _window, cx| {
                app.user_menu = false;
                app.section = Section::Studio;
                app.studio.update(cx, |panel, cx| {
                    panel.open_session(&open_id, cx);
                });
                cx.notify();
            }))
    }

    /// The conversation's only right-hand button. It sits in the same slot as
    /// the project's edit button, the rightmost action.
    fn conversation_row_actions(
        &self,
        _window: &mut Window,
        cx: &Context<'_, Self>,
        session_id: &str,
    ) -> impl IntoElement {
        let id = session_id.to_owned();
        div()
            .id(format!("task-actions-{session_id}"))
            .flex()
            .items_center()
            .flex_shrink_0()
            .debug_selector(|| "task-actions".into())
            .child(theme::anchor_trigger(
                dock_icon_button(
                    format!("task-options-hit-{session_id}"),
                    theme::IconButton::new(
                        format!("task-options-{session_id}"),
                        self.t("library.more"),
                        theme::icons::ellipsis(),
                    )
                    .disabled(self.conversation_delete_busy()),
                    &self.theme,
                    cx.listener({
                        move |app, _, _, cx| {
                            cx.stop_propagation();
                            app.toggle_task_menu(&id, cx);
                        }
                    }),
                ),
                format!("task-menu-trigger-{session_id}"),
                &self.ui,
            ))
    }

    fn task_rows(
        &self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
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
            rows = rows.child(self.conversation_row(
                window,
                cx,
                id,
                session_id,
                title,
                selected,
                DockIcon::Task.render(),
                "task-row",
            ));
        }
        rows
    }

    fn project_section(
        &self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let open = self.projects_open;
        let toggle =
            cx.listener(|app, _event: &gpui::ClickEvent, _window, cx| {
                app.toggle_projects_section(cx);
            });
        let mut section = div().child(self.section_header(
            window,
            cx,
            "project-group-header",
            "dock.projects",
            open,
            true,
            toggle,
        ));
        if open {
            section = section.child(self.project_rows(window, cx));
        }
        section
    }

    fn task_section(
        &self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let open = self.tasks_open;
        let toggle =
            cx.listener(|app, _event: &gpui::ClickEvent, _window, cx| {
                app.toggle_tasks_section(cx);
            });
        let mut section = div().child(self.section_header(
            window,
            cx,
            "task-group-header",
            "dock.tasks",
            open,
            false,
            toggle,
        ));
        if open {
            section = section.child(self.task_rows(window, cx));
        }
        section
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "one row reads better than a struct built at each call site"
    )]
    fn section_header(
        &self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
        id: &'static str,
        title_key: &'static str,
        open: bool,
        add_project: bool,
        on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    ) -> impl IntoElement {
        let hovered = window.use_keyed_state(
            gpui::SharedString::from(format!("{id}-hovered")),
            cx,
            |_, _| false,
        );
        let row_hovered = *hovered.read(cx);
        let show_add = add_project && row_hovered;
        let line = body_line_height(&self.theme, window);
        let title = self.t(title_key);
        let toggle_label = self.tf(
            if open { "dock.collapse" } else { "dock.expand" },
            &[("name", title.as_str())],
        );
        let header = theme::interactive(id, toggle_label, &self.theme)
            .mx_2()
            .px(self.theme.space(8.))
            .py(self.theme.space(4.))
            .rounded_md()
            .flex()
            .items_center()
            .gap_2()
            .bg(self.theme.dock())
            .hover(|row| row.bg(self.theme.dock_selected()))
            .on_hover({
                let hovered = hovered;
                move |active, window, cx| {
                    let changed = hovered.update(cx, |state, cx| {
                        if *state == *active {
                            return false;
                        }
                        *state = *active;
                        cx.notify();
                        true
                    });
                    if changed {
                        window.refresh();
                    }
                }
            })
            .debug_selector(move || id.into())
            .on_click(on_click)
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(self.theme.caption_size())
                            .line_height(line)
                            .text_color(self.theme.muted())
                            .child(title),
                    )
                    .when(row_hovered, |label| {
                        let disclosure = if open {
                            theme::icons::chevron_down()
                        } else if self.i18n.locale().is_rtl() {
                            theme::icons::chevron_left()
                        } else {
                            theme::icons::chevron_right()
                        };
                        label.child(
                            disclosure
                                .size(px(12.))
                                .text_color(self.theme.muted()),
                        )
                    }),
            )
            .when(show_add, |row| {
                row.child(dock_icon_button(
                    "new-project-hit",
                    theme::IconButton::new(
                        "new-project",
                        self.t("dock.new_project"),
                        theme::icons::plus(),
                    ),
                    &self.theme,
                    cx.listener(|app, _event, _window, cx| {
                        cx.stop_propagation();
                        app.user_menu = false;
                        app.open_project_create(cx);
                    }),
                ))
            });
        div().pt_4().child(header)
    }

    pub(super) fn toggle_projects_section(
        &mut self,
        cx: &mut Context<'_, Self>,
    ) {
        self.user_menu = false;
        self.projects_open = !self.projects_open;
        cx.notify();
    }

    pub(super) fn toggle_tasks_section(&mut self, cx: &mut Context<'_, Self>) {
        self.user_menu = false;
        self.tasks_open = !self.tasks_open;
        cx.notify();
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
                .px_2()
                .py(px(1.))
                .rounded_md()
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

/// Same inset, padding, border, and radius as a dock navigation row.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "every theme helper in this dock module takes the theme by reference"
)]
fn dock_nav_row(
    theme: &flowy_ui::ResolvedTheme,
    id: impl Into<gpui::ElementId>,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .border_2()
        .border_color(theme.line().opacity(0.))
        .mx_2()
        .px(theme.space(8.))
        .py(theme.space(4.))
        .rounded_md()
        .flex()
        .items_center()
        .gap_2()
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "every theme helper in this dock module takes the theme by reference"
)]
fn body_line_height(
    theme: &flowy_ui::ResolvedTheme,
    window: &Window,
) -> gpui::Pixels {
    // `gpui-pre` keeps the golden ratio as a `DefiniteLength` fraction; the
    // plain constant is what the old `DefiniteLength::as_f32` handed back.
    window.pixel_snap(px(
        theme.body_size().as_f32() * std::f32::consts::GOLDEN_RATIO
    ))
}

/// A dock action icon.
///
/// `IconButton::render` erases to `AnyElement`, which no longer exposes the
/// interactive surface, so the hit target that the anchor and the tooltip
/// attach to lives on this shell. The control keeps its own `ControlSize`
/// metrics instead of the old line-height square.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "every theme helper in this dock module takes the theme by reference"
)]
fn dock_icon_button(
    id: impl Into<gpui::ElementId>,
    button: theme::IconButton,
    theme: &flowy_ui::ResolvedTheme,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    div().id(id).flex_shrink_0().child(
        button
            .size(theme::ControlSize::Small)
            .render(theme, on_click),
    )
}

/// Project folder toggle. Same hit target as the other dock icons, with no
/// fill of its own, so hovering it stays on the row background.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "every theme helper in this dock module takes the theme by reference"
)]
fn project_folder_toggle(
    window: &Window,
    theme: &flowy_ui::ResolvedTheme,
    id: impl Into<gpui::ElementId>,
    label: impl Into<gpui::SharedString>,
    icon: impl IntoElement,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> flowy_ui::InteractiveDiv {
    let line = body_line_height(theme, window);
    theme::interactive(id, label, theme)
        .w(line)
        .h(line)
        .min_w(line)
        .min_h(line)
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .on_click(on_click)
        .child(icon)
}

fn item_row(
    theme: flowy_ui::ResolvedTheme,
    id: impl Into<gpui::ElementId>,
    label: String,
    selected: bool,
    icon: DockIcon,
) -> flowy_ui::InteractiveDiv {
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
