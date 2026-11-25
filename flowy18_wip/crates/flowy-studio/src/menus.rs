//! Local environment status popover and cockpit picker menus (workspace, skill).

use flowy_ui as theme;
use gpui::{Context, FontWeight, IntoElement, Stateful, div, prelude::*, px};

use crate::overview::status_pill;
use crate::panel::StudioPanel;

impl StudioPanel {
    fn popover_card(
        &self,
        id: &'static str,
        width: f32,
        padding: f32,
        gap: f32,
    ) -> Stateful<gpui::Div> {
        div()
            .id(id)
            .occlude()
            .w(px(width))
            .p(self.theme.space(padding))
            .rounded_xl()
            .bg(self.theme.card())
            .border_1()
            .border_color(self.theme.line())
            .shadow_md()
            .flex()
            .flex_col()
            .gap(px(gap))
    }

    pub(crate) fn env_status_popover(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let env = &self.local_env;
        let l3_enabled = env.semantic_verify_enabled;
        let ready = matches!(env.overall, crate::ports::EnvSeverity::Ready);
        let (pill_bg, pill_fg, pill_text) = if ready {
            (
                self.theme.status_verified_bg(),
                self.theme.status_verified(),
                self.t("env.pill_ready"),
            )
        } else {
            (
                self.theme.status_attention_bg(),
                self.theme.status_attention(),
                self.t("env.pill_attention"),
            )
        };
        let mut card = self
            .popover_card("env-status-card", 300.0, 14.0, 8.0)
            .child(
                div()
                    .pb(self.theme.space(6.))
                    .border_b_1()
                    .border_color(self.theme.line())
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.ink())
                            .child(self.t("env.popover_title")),
                    )
                    .child(status_pill(pill_bg, pill_fg, pill_text, None)),
            );

        for check in &env.checks {
            let color = match check.severity {
                crate::ports::EnvSeverity::Ready => {
                    self.theme.status_verified()
                }
                crate::ports::EnvSeverity::Warning => {
                    self.theme.status_attention()
                }
            };
            card = card.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .child(
                        div()
                            .text_color(self.theme.muted())
                            .child(self.t(check.label_key)),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(color)
                            .child(check.detail.resolve(|k| self.t(k))),
                    ),
            );
        }

        card.child(
            div()
                .pt(self.theme.space(8.))
                .border_t_1()
                .border_color(self.theme.line())
                .flex()
                .items_center()
                .justify_between()
                .gap(self.theme.space(8.))
                .child(
                    div()
                        .text_xs()
                        .text_color(self.theme.ink())
                        .child(self.t("env.toggle_l3_label")),
                )
                .child(
                    theme::Button::new(
                        "env-toggle-l3",
                        if l3_enabled {
                            self.t("env.state_on")
                        } else {
                            self.t("env.state_off")
                        },
                    )
                    .variant(if l3_enabled {
                        theme::ControlVariant::Primary
                    } else {
                        theme::ControlVariant::Secondary
                    })
                    .size(theme::ControlSize::Small)
                    .render(
                        &self.theme,
                        cx.listener(|panel, _, _, cx| {
                            panel.toggle_semantic_verify(cx);
                        }),
                    ),
                ),
        )
    }

    pub(crate) fn workspace_menu(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let mut menu = self
            .popover_card("workspace-menu-panel", 260.0, 8.0, 4.0)
            .max_h(px(280.))
            .overflow_y_scroll();

        let default_selected = self.active_project_id.is_none();
        menu = menu.child(
            theme::interactive(
                "workspace-opt-default",
                self.t("composer.workspace_default"),
                &self.theme,
            )
            .w_full()
            .px(self.theme.space(10.))
            .py(px(6.))
            .rounded_lg()
            .cursor_pointer()
            .hover(|style| style.bg(self.theme.soft()))
            .flex()
            .items_center()
            .justify_between()
            .text_xs()
            .child(self.t("composer.workspace_default"))
            .when(default_selected, |row| row.child(theme::icons::check()))
            .on_click(cx.listener(|panel, _, _, cx| {
                panel.workspace_menu_open = false;
                panel.bind_default_workspace(cx);
            })),
        );

        for (idx, project) in self.projects.iter().enumerate() {
            let pid = project.id.clone();
            let selected =
                self.active_project_id.as_deref() == Some(pid.as_str());
            let path_label = crate::ports::display_path(&project.root);
            menu = menu.child(
                theme::interactive(
                    ("workspace-opt-proj", idx as u64),
                    project.name.clone(),
                    &self.theme,
                )
                .w_full()
                .px(self.theme.space(10.))
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
                )
                .when(selected, |row| row.child(theme::icons::check()))
                .on_click(cx.listener(
                    move |panel, _, _, cx| {
                        panel.workspace_menu_open = false;
                        let _ = panel.open_project(&pid, cx);
                    },
                )),
            );
        }
        menu
    }

    pub(crate) fn skill_menu(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let mut menu = self
            .popover_card("skill-menu-panel", 280.0, 8.0, 4.0)
            .max_h(px(300.))
            .overflow_y_scroll();

        if self.selected_skill.is_some() {
            menu = menu.child(
                theme::interactive(
                    "skill-opt-clear",
                    self.t("composer.skill_clear"),
                    &self.theme,
                )
                .w_full()
                .px(self.theme.space(10.))
                .py(px(6.))
                .rounded_lg()
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.soft()))
                .text_xs()
                .text_color(self.theme.muted())
                .child(self.t("composer.skill_clear"))
                .on_click(cx.listener(|panel, _, _, cx| {
                    panel.selected_skill = None;
                    panel.skill_menu_open = false;
                    cx.notify();
                })),
            );
        }

        if self.installed_skills.is_empty() {
            menu = menu.child(
                div()
                    .p(self.theme.space(10.))
                    .text_xs()
                    .text_color(self.theme.muted())
                    .child(self.t("composer.skill_empty")),
            );
        } else {
            for (idx, skill) in self.installed_skills.iter().enumerate() {
                let chip = skill.clone();
                let selected = self
                    .selected_skill
                    .as_ref()
                    .is_some_and(|cur| cur.id == skill.id);
                menu = menu.child(
                    theme::interactive(
                        ("skill-opt-item", idx as u64),
                        skill.title.clone(),
                        &self.theme,
                    )
                    .w_full()
                    .px(self.theme.space(10.))
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
                    )
                    .when(selected, |row| row.child(theme::icons::check()))
                    .on_click(cx.listener(
                        move |panel, _, _, cx| {
                            panel.selected_skill = Some(chip.clone());
                            panel.skill_menu_open = false;
                            cx.notify();
                        },
                    )),
                );
            }
        }
        menu
    }
}
