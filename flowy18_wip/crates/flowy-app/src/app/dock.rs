use super::{FlowyApp, Section};
use flowy_ui as theme;
use gpui::{
    Animation, AnimationExt, Context, SharedString, Window, div, prelude::*, px,
};

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
        let (
            active_work,
            in_chat,
            chief_selected,
            new_task_selected,
            chief_preview,
            busy_ids,
        ) = {
            let studio = self.studio.read(cx);
            let in_chat = self.section == Section::Studio
                && studio.surface == flowy_studio::StudioSurface::Chat;
            let default_preview = studio
                .agent
                .list_sessions()
                .into_iter()
                .find(|session| session.project_id.is_none())
                .and_then(|session| studio.agent.session_preview(&session.id));
            (
                studio.background_work_count(),
                in_chat,
                in_chat && studio.active_project_id.is_none(),
                self.section == Section::Studio && !in_chat,
                default_preview,
                studio.agent.busy_session_ids(),
            )
        };
        let mut navigation =
            div().flex().flex_col().gap_1().child(Self::nav_item(
                &self.theme,
                "new-task",
                self.t("dock.new_task"),
                new_task_selected,
                Section::Studio,
                DockIcon::NewTask,
                cx,
            ));

        // Primary space navigation (Library stays prominent); secondary tools sit
        // quieter below a subtle hairline divider.
        navigation = navigation.child(Self::nav_item(
            &self.theme,
            "library",
            self.t("dock.library"),
            self.section == Section::Library,
            Section::Library,
            DockIcon::Library,
            cx,
        ));
        let mut secondary = div()
            .mt_1()
            .pt_1()
            .mx_2()
            .border_t_1()
            .border_color(self.theme.line())
            .flex()
            .flex_col()
            .gap(px(2.));
        for (id, key, section, icon) in NAV {
            if *id == "library" {
                continue;
            }
            let selected = self.section == *section;
            secondary = secondary.child(Self::quiet_nav_item(
                &self.theme,
                id,
                self.t(key),
                selected,
                *section,
                *icon,
                cx,
            ));
        }
        secondary = secondary
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
            .child(self.chief_assistant_card(
                active_work,
                in_chat,
                chief_selected,
                chief_preview,
                window,
                cx,
            ))
            .child(navigation)
            .child(self.project_section(&busy_ids, window, cx))
            .child(self.task_section(&busy_ids, window, cx))
            .child(secondary)
    }

    fn chief_assistant_card(
        &self,
        active_work: usize,
        in_chat: bool,
        selected: bool,
        latest_preview: Option<String>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let hovered_state =
            window.use_keyed_state("dock-chief-hovered", cx, |_, _| false);
        let hovered = *hovered_state.read(cx);
        let scale = theme::motion::travel_scale(self.theme.motion_policy());
        let badge_core = div()
            .size(px(34.))
            .flex_shrink_0()
            .rounded_xl()
            .bg(self.theme.assistant_badge_bg())
            .text_color(self.theme.assistant_badge_fg())
            .shadow_sm()
            .flex()
            .items_center()
            .justify_center()
            .child(
                theme::icons::sparkles()
                    .size(px(15.))
                    .text_color(self.theme.assistant_badge_fg()),
            );
        let badge = if (hovered || active_work > 0) && scale > 0.0 {
            badge_core
                .with_animation(
                    "dock-chief-pulse",
                    Animation::new(theme::motion::PULSE)
                        .repeat()
                        .with_easing(theme::motion::pulsing_sine()),
                    move |el, t| {
                        el.opacity(0.14_f32.mul_add(t, 0.86))
                            .relative()
                            .top(px(-1.5 * scale * t))
                    },
                )
                .into_any_element()
        } else {
            badge_core.into_any_element()
        };

        let title = self.t("dock.chief_assistant");
        let subtitle_text = latest_preview.unwrap_or_else(|| {
            if active_work > 0 {
                let count = active_work.to_string();
                self.tf("dock.chief_working", &[("count", count.as_str())])
            } else {
                self.t("dock.chief_idle")
            }
        });
        let subtitle = div()
            .flex()
            .items_center()
            .gap(px(5.))
            .min_w_0()
            .when(active_work > 0, |row| {
                row.child(running_dot("dock-chief-active-dot", self.theme))
            })
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .truncate()
                    .text_size(self.theme.caption_size())
                    .text_color(self.theme.muted())
                    .child(subtitle_text),
            );

        let hover_bg = if selected {
            self.theme.card()
        } else {
            self.theme.dock_hover()
        };

        theme::interactive("dock-chief-assistant", title.clone(), &self.theme)
            .mx_2()
            .mb_2()
            .px(self.theme.space(8.))
            .py(px(8.))
            .rounded_xl()
            .border_1()
            .border_color(self.theme.line())
            .flex()
            .items_center()
            .gap(self.theme.space(8.))
            .bg(if selected {
                self.theme.card()
            } else {
                self.theme.dock()
            })
            .when(selected, gpui::Styled::shadow_sm)
            .hover(move |card| card.bg(hover_bg))
            .on_hover(move |active, window, cx| {
                let changed = hovered_state.update(cx, |state, cx| {
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
            })
            .on_click(cx.listener(|app, _event, _window, cx| {
                app.user_menu = false;
                app.section = Section::Studio;
                app.studio.update(
                    cx,
                    flowy_studio::StudioPanel::open_chief_assistant,
                );
                cx.notify();
            }))
            .child(badge)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(4.))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .truncate()
                                    .text_size(self.theme.body_size())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(self.theme.ink())
                                    .child(title),
                            )
                            .when(active_work > 0, |row| {
                                row.child(
                                    div()
                                        .flex_none()
                                        .px(px(5.))
                                        .py(px(1.))
                                        .rounded_full()
                                        .bg(self.theme.status_active_bg())
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(self.theme.status_active())
                                        .child(active_work.to_string()),
                                )
                            }),
                    )
                    .child(subtitle),
            )
            .when(in_chat, |card| {
                let fold_label = self.t("dock.fold_desk");
                let on_fold = cx.listener(|app, _event, _window, cx| {
                    cx.stop_propagation();
                    app.user_menu = false;
                    app.studio.update(cx, flowy_studio::StudioPanel::fold_desk);
                    cx.notify();
                });
                card.child(theme::tooltip_trigger(
                    "dock-fold-desk-tip",
                    dock_icon_button(
                        "dock-fold-desk-hit",
                        theme::IconButton::new(
                            "dock-fold-desk",
                            fold_label.clone(),
                            theme::icons::minus(),
                        ),
                        &self.theme,
                        on_fold,
                    ),
                    fold_label,
                    &self.ui,
                    window,
                    cx,
                    self.i18n.locale().is_rtl(),
                ))
            })
    }

    fn quiet_nav_item(
        theme: &flowy_ui::ResolvedTheme,
        id: &'static str,
        label: String,
        selected: bool,
        section: Section,
        icon: DockIcon,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        quiet_row(*theme, id, label, selected, icon).on_click(cx.listener(
            move |app, _event, _window, cx| {
                app.select_navigation(id, section, cx);
            },
        ))
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
        quiet_row(self.theme, id, self.t(key), selected, icon).on_click(
            cx.listener(move |app, _event, _window, cx| {
                app.user_menu = false;
                app.select_more_page(page, cx);
            }),
        )
    }

    fn project_rows(
        &self,
        busy_ids: &[String],
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let (mut projects, sessions, active_session, attention_ids) = {
            let studio = self.studio.read(cx);
            let sessions = studio.agent.list_sessions();
            let attention_ids: std::collections::HashSet<String> = sessions
                .iter()
                .filter(|s| studio.agent.session_needs_attention(&s.id))
                .map(|s| s.id.clone())
                .collect();
            (
                studio.projects.clone(),
                sessions,
                studio.agent.active_session_id(),
                attention_ids,
            )
        };
        projects.sort_by_key(|project| !project.pinned);
        let project_ids: Vec<String> =
            projects.iter().map(|project| project.id.clone()).collect();
        let flip = window.use_keyed_state("dock-project-flip", cx, |_, _| {
            theme::motion::FlipState::default()
        });
        flip.update(cx, |state, _| state.reconcile(&project_ids, 36.0));
        let policy = self.theme.motion_policy();
        let mut rows = div().mt(px(2.)).flex().flex_col().gap(px(2.));
        for project in projects {
            let project_id = project.id.clone();
            let expanded = self.expanded_projects.contains(&project_id);
            let menu_open =
                self.project_menu.as_deref() == Some(project_id.as_str());
            let project_busy = sessions.iter().any(|session| {
                session.project_id.as_deref() == Some(project_id.as_str())
                    && busy_ids.contains(&session.id)
            });
            let project_attention = !project_busy
                && sessions.iter().any(|session| {
                    session.project_id.as_deref() == Some(project_id.as_str())
                        && attention_ids.contains(&session.id)
                });
            let flip_offset = flip.read(cx).offset_for(&project_id);
            let toggle_label = if expanded {
                self.tf("project.collapse", &[("name", &project.name)])
            } else {
                self.tf("project.expand", &[("name", &project.name)])
            };
            let hovered = window.use_keyed_state(
                SharedString::from(format!("project-row-hovered-{project_id}")),
                cx,
                |_, _| false,
            );
            let row_hovered = *hovered.read(cx);
            let show_actions = row_hovered || menu_open;
            let monogram = project.name.trim().chars().next().map_or_else(
                || "S".to_owned(),
                |c| c.to_uppercase().to_string(),
            );
            // The monogram emblem stays put at rest and swaps cleanly to the
            // disclosure chevron on hover, keeping exact row dimensions.
            let toggle_icon = if row_hovered {
                if expanded {
                    theme::icons::chevron_down()
                        .size(px(13.))
                        .text_color(self.theme.muted())
                        .into_any_element()
                } else {
                    theme::icons::chevron_right()
                        .size(px(13.))
                        .text_color(self.theme.muted())
                        .into_any_element()
                }
            } else {
                div()
                    .size(px(18.))
                    .rounded(px(5.))
                    .bg(self.theme.soft())
                    .border_1()
                    .border_color(self.theme.line())
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(self.theme.ink())
                    .child(monogram)
                    .into_any_element()
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
                        .gap(px(6.))
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
                        .when(project_busy, |row| {
                            row.child(running_dot(
                                SharedString::from(format!(
                                    "dock-project-running-{project_id}"
                                )),
                                self.theme,
                            ))
                        })
                        .when(project_attention, |row| {
                            row.child(attention_dot(self.theme))
                        })
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
            let row_el = theme::motion::flip_row(
                SharedString::from(format!(
                    "project-flip-{project_id}-{}",
                    flip_offset.map_or(0, |(epoch, _)| epoch)
                )),
                flip_offset,
                policy,
                row,
            );
            rows = rows.child(row_el);

            if expanded {
                for session in sessions.iter().filter(|session| {
                    session.project_id.as_deref() == Some(project_id.as_str())
                }) {
                    let busy = busy_ids.contains(&session.id);
                    let attention =
                        !busy && attention_ids.contains(&session.id);
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
                        busy,
                        attention,
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
        busy: bool,
        attention: bool,
        leading: impl IntoElement,
        selector: &'static str,
    ) -> impl IntoElement {
        let menu_open = self.task_menu.as_deref() == Some(session_id.as_str());
        let hovered = window.use_keyed_state(
            SharedString::from(format!(
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
            .when(busy, |row| {
                row.child(running_dot(
                    SharedString::from(format!("dock-running-{session_id}")),
                    self.theme,
                ))
            })
            .when(attention && !busy, |row| {
                row.child(attention_dot(self.theme))
            })
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
        busy_ids: &[String],
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let (sessions, active, attention_ids) = {
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
            let attention_ids: std::collections::HashSet<String> = sessions
                .iter()
                .filter(|s| studio.agent.session_needs_attention(&s.id))
                .map(|s| s.id.clone())
                .collect();
            (sessions, studio.agent.active_session_id(), attention_ids)
        };
        if sessions.is_empty() {
            return div()
                .px_4()
                .py_1()
                .text_size(self.theme.body_size())
                .text_color(self.theme.muted())
                .child(self.t("dock.no_tasks"));
        }
        let session_ids: Vec<String> =
            sessions.iter().map(|s| s.id.clone()).collect();
        let flip = window.use_keyed_state("dock-task-flip", cx, |_, _| {
            theme::motion::FlipState::default()
        });
        flip.update(cx, |state, _| state.reconcile(&session_ids, 32.0));
        let policy = self.theme.motion_policy();
        let mut rows = div().flex().flex_col();
        for (index, session) in sessions.into_iter().enumerate() {
            let id = format!("task-{index}");
            let session_id = session.id;
            let title = if session.title.is_empty() {
                self.t("dock.new_task")
            } else {
                session.title
            };
            let selected = active.as_deref() == Some(session_id.as_str());
            let busy = busy_ids.contains(&session_id);
            let attention = !busy && attention_ids.contains(&session_id);
            let flip_offset = flip.read(cx).offset_for(&session_id);
            let row = self.conversation_row(
                window,
                cx,
                id,
                session_id.clone(),
                title,
                selected,
                busy,
                attention,
                DockIcon::Task.render(),
                "task-row",
            );
            let row_el = theme::motion::flip_row(
                SharedString::from(format!(
                    "task-flip-{session_id}-{}",
                    flip_offset.map_or(0, |(epoch, _)| epoch)
                )),
                flip_offset,
                policy,
                row,
            );
            rows = rows.child(row_el);
        }
        rows
    }

    fn project_section(
        &self,
        busy_ids: &[String],
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
            section = section.child(self.project_rows(busy_ids, window, cx));
        }
        section
    }

    fn task_section(
        &self,
        busy_ids: &[String],
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
            section = section.child(self.task_rows(busy_ids, window, cx));
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

fn quiet_row(
    theme: flowy_ui::ResolvedTheme,
    id: &'static str,
    label: String,
    selected: bool,
    icon: DockIcon,
) -> flowy_ui::InteractiveDiv {
    theme::interactive(id, label.clone(), &theme)
        .px(theme.space(8.))
        .py(theme.space(4.))
        .rounded_md()
        .flex()
        .items_center()
        .gap_2()
        .bg(if selected {
            theme.dock_selected()
        } else {
            theme.dock()
        })
        .text_color(if selected { theme.ink() } else { theme.muted() })
        .hover(|row| row.bg(theme.dock_hover()).text_color(theme.ink()))
        .child(icon.render().size(px(14.)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(theme.caption_size())
                .child(label),
        )
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

fn running_dot(
    id: impl Into<gpui::ElementId>,
    theme: theme::ResolvedTheme,
) -> gpui::AnyElement {
    theme::motion::live_dot(id, theme.status_active(), theme.motion_policy())
}

fn attention_dot(theme: theme::ResolvedTheme) -> impl IntoElement {
    div()
        .size(px(6.))
        .flex_shrink_0()
        .rounded_full()
        .bg(theme.status_attention())
}
