//! Unified `+` composer menu: Working Folder, Installed Skills, Connectors,
//! and Preferences (replacing 3 noisy composer chrome chips with one calm menu).

use flowy_ui as theme;
use gpui::{Context, FontWeight, IntoElement, div, prelude::*, px};

use crate::panel::StudioPanel;

impl StudioPanel {
    pub(crate) fn composer_add_menu(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        self.popover_card("composer-add-menu-panel", 296.0, 8.0, 4.0)
            .max_h(px(380.))
            .overflow_y_scroll()
            .child(self.workspace_section(cx))
            .child(self.skills_section(cx))
            .child(self.connectors_section())
            .child(self.preferences_section(cx))
    }

    fn add_menu_section_heading(&self, key: &str) -> impl IntoElement {
        div()
            .px(self.theme.space(8.))
            .pt(px(4.))
            .pb(px(2.))
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(self.theme.muted())
            .child(self.t(key))
    }

    fn workspace_section(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut section = div().flex().flex_col().gap(px(2.)).child(
            self.add_menu_section_heading("composer.add_workspace_heading"),
        );

        let default_selected = self.active_project_id.is_none();
        section = section.child(
            theme::interactive(
                "workspace-opt-default",
                self.t("composer.workspace_default"),
                &self.theme,
            )
            .w_full()
            .px(self.theme.space(8.))
            .py(px(6.))
            .rounded_lg()
            .cursor_pointer()
            .hover(|style| style.bg(self.theme.soft()))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(6.))
            .text_xs()
            .text_color(self.theme.ink())
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        theme::icons::folder()
                            .size(px(13.))
                            .text_color(self.theme.muted()),
                    )
                    .child(self.t("composer.workspace_default")),
            )
            .when(default_selected, |row| {
                row.child(
                    theme::icons::check()
                        .size(px(13.))
                        .text_color(self.theme.status_active()),
                )
            })
            .on_click(cx.listener(|panel, _, _, cx| {
                panel.close_all_popovers();
                panel.bind_default_workspace(cx);
            })),
        );

        for (idx, project) in self.projects.iter().enumerate() {
            let pid = project.id.clone();
            let selected =
                self.active_project_id.as_deref() == Some(pid.as_str());
            let path_label = crate::ports::display_path(&project.root);
            section = section.child(
                theme::interactive(
                    ("workspace-opt-proj", idx as u64),
                    project.name.clone(),
                    &self.theme,
                )
                .w_full()
                .px(self.theme.space(8.))
                .py(px(6.))
                .rounded_lg()
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.soft()))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(6.))
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            theme::icons::folder_open()
                                .size(px(13.))
                                .text_color(self.theme.muted()),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(self.theme.ink())
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .truncate()
                                        .child(project.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(self.theme.muted())
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .truncate()
                                        .child(path_label),
                                ),
                        ),
                )
                .when(selected, |row| {
                    row.child(
                        theme::icons::check()
                            .size(px(13.))
                            .text_color(self.theme.status_active()),
                    )
                })
                .on_click(cx.listener(
                    move |panel, _, _, cx| {
                        panel.close_all_popovers();
                        let _ = panel.open_project(&pid, cx);
                    },
                )),
            );
        }
        section
    }

    fn skills_section(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut section = div()
            .mt(px(4.))
            .pt(px(4.))
            .border_t_1()
            .border_color(self.theme.line())
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(
                self.add_menu_section_heading("composer.add_skills_heading"),
            );

        if self.selected_skill.is_some() {
            section = section.child(
                theme::interactive(
                    "skill-opt-clear",
                    self.t("composer.skill_clear"),
                    &self.theme,
                )
                .w_full()
                .px(self.theme.space(8.))
                .py(px(5.))
                .rounded_lg()
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.soft()))
                .text_xs()
                .text_color(self.theme.muted())
                .child(self.t("composer.skill_clear"))
                .on_click(cx.listener(|panel, _, _, cx| {
                    panel.selected_skill = None;
                    panel.close_all_popovers();
                    cx.notify();
                })),
            );
        }

        if self.installed_skills.is_empty() {
            return section.child(
                div()
                    .px(self.theme.space(8.))
                    .py(px(6.))
                    .text_xs()
                    .text_color(self.theme.muted())
                    .child(self.t("composer.skill_empty")),
            );
        }

        for (idx, skill) in self.installed_skills.iter().enumerate() {
            let chip = skill.clone();
            let selected = self
                .selected_skill
                .as_ref()
                .is_some_and(|cur| cur.id == skill.id);
            section = section.child(
                theme::interactive(
                    ("skill-opt-item", idx as u64),
                    skill.title.clone(),
                    &self.theme,
                )
                .w_full()
                .px(self.theme.space(8.))
                .py(px(6.))
                .rounded_lg()
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.soft()))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(6.))
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            theme::icons::sparkles()
                                .size(px(13.))
                                .text_color(self.theme.muted()),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(self.theme.ink())
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .truncate()
                                        .child(skill.title.clone()),
                                )
                                .when(!skill.summary.is_empty(), |col| {
                                    col.child(
                                        div()
                                            .text_xs()
                                            .text_color(self.theme.muted())
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .truncate()
                                            .child(skill.summary.clone()),
                                    )
                                }),
                        ),
                )
                .when(selected, |row| {
                    row.child(
                        theme::icons::check()
                            .size(px(13.))
                            .text_color(self.theme.status_active()),
                    )
                })
                .on_click(cx.listener(
                    move |panel, _, _, cx| {
                        panel.selected_skill = Some(chip.clone());
                        panel.close_all_popovers();
                        cx.notify();
                    },
                )),
            );
        }
        section
    }

    fn connectors_section(&self) -> impl IntoElement {
        div()
            .mt(px(4.))
            .pt(px(4.))
            .border_t_1()
            .border_color(self.theme.line())
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(
                self.add_menu_section_heading(
                    "composer.add_connectors_heading",
                ),
            )
            .child(
                div()
                    .w_full()
                    .px(self.theme.space(8.))
                    .py(px(6.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(6.))
                    .text_xs()
                    .text_color(self.theme.muted())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                theme::icons::plug()
                                    .size(px(13.))
                                    .text_color(self.theme.muted()),
                            )
                            .child(self.t("composer.add_connectors_heading")),
                    )
                    .child(
                        div()
                            .px(px(6.))
                            .py(px(1.))
                            .rounded_full()
                            .bg(self.theme.soft())
                            .text_xs()
                            .text_color(self.theme.muted())
                            .child(self.t("composer.add_connectors_soon")),
                    ),
            )
    }

    fn preferences_section(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let verify_on = self.agent.semantic_verify_enabled();
        let verify_label = self.t("env.toggle_l3_label");
        let verify_state = if verify_on {
            self.t("env.state_on")
        } else {
            self.t("env.state_off")
        };
        div()
            .mt(px(4.))
            .pt(px(4.))
            .border_t_1()
            .border_color(self.theme.line())
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(self.add_menu_section_heading("composer.add_prefs_heading"))
            .child(
                theme::interactive(
                    "composer-add-verify-toggle",
                    format!("{verify_label} \u{00b7} {verify_state}"),
                    &self.theme,
                )
                .w_full()
                .px(self.theme.space(8.))
                .py(px(6.))
                .rounded_lg()
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.soft()))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .text_xs()
                        .text_color(self.theme.ink())
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .truncate()
                        .child(verify_label),
                )
                .child(
                    div()
                        .flex_none()
                        .px(px(8.))
                        .py(px(2.))
                        .rounded_full()
                        .border_1()
                        .border_color(if verify_on {
                            self.theme.status_verified().opacity(0.45)
                        } else {
                            self.theme.line()
                        })
                        .bg(if verify_on {
                            self.theme.status_verified_bg()
                        } else {
                            self.theme.soft()
                        })
                        .flex()
                        .items_center()
                        .gap(px(3.))
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(if verify_on {
                            self.theme.status_verified()
                        } else {
                            self.theme.muted()
                        })
                        .when(verify_on, |pill| {
                            pill.child(theme::icons::check().size(px(10.)))
                        })
                        .child(verify_state),
                )
                .on_click(cx.listener(|panel, _, _, cx| {
                    panel.toggle_semantic_verify(cx);
                })),
            )
    }
}
