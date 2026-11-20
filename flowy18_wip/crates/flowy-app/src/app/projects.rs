use super::{FlowyApp, Section, state};
use flowy_ui as theme;
use gpui::{Context, Focusable, Pixels, TextRun, Window, div, prelude::*, px};

impl FlowyApp {
    pub(super) fn open_project_create(&mut self, cx: &mut Context<'_, Self>) {
        self.project_create.picker_generation =
            self.project_create.picker_generation.wrapping_add(1);
        self.project_create.folder = None;
        self.project_create.status = None;
        self.project_create.editing_project_id = None;
        self.project_create.deleting_project_id = None;
        self.project_create.title_key = "project.create_title";
        self.project_menu = None;
        self.project_create
            .name
            .update(cx, |state, cx| state.emplace("", cx));
        self.activate_dialog(state::ActiveDialog::ProjectCreate, cx);
    }

    pub(super) fn open_project(
        &mut self,
        project_id: &str,
        cx: &mut Context<'_, Self>,
    ) {
        self.user_menu = false;
        self.expanded_projects.insert(project_id.to_owned());
        self.push_section(Section::Studio, cx);
        self.studio.update(cx, |panel, cx| {
            let _ = panel.open_project(project_id, cx);
        });
    }

    pub(super) fn edit_project(
        &mut self,
        project_id: &str,
        cx: &mut Context<'_, Self>,
    ) {
        let project = self
            .studio
            .read(cx)
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .cloned();
        let Some(project) = project else {
            return;
        };
        self.project_create.picker_generation =
            self.project_create.picker_generation.wrapping_add(1);
        self.project_create.folder = Some(project.root);
        self.project_create.status = None;
        self.project_create.editing_project_id = Some(project.id);
        self.project_create.title_key = "project.edit_title";
        self.project_create
            .name
            .update(cx, |state, cx| state.emplace(&project.name, cx));
        self.project_menu = None;
        self.activate_dialog(state::ActiveDialog::ProjectCreate, cx);
    }

    pub(super) fn copy_project(
        &mut self,
        project_id: &str,
        cx: &mut Context<'_, Self>,
    ) {
        let project = self
            .studio
            .read(cx)
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .cloned();
        let Some(project) = project else {
            return;
        };
        self.project_create.picker_generation =
            self.project_create.picker_generation.wrapping_add(1);
        self.project_create.folder = None;
        self.project_create.status = None;
        self.project_create.editing_project_id = None;
        self.project_create.title_key = "project.copy_title";
        let copy_name =
            format!("{} {}", project.name, self.t("project.copy_suffix"));
        self.project_create
            .name
            .update(cx, |state, cx| state.emplace(&copy_name, cx));
        self.project_menu = None;
        self.activate_dialog(state::ActiveDialog::ProjectCreate, cx);
    }

    pub(super) fn toggle_project_menu(
        &mut self,
        id: &str,
        cx: &mut Context<'_, Self>,
    ) {
        self.user_menu = false;
        self.project_menu =
            (self.project_menu.as_deref() != Some(id)).then(|| id.to_owned());
        cx.notify();
    }

    pub(super) fn toggle_project_expanded(
        &mut self,
        id: &str,
        cx: &mut Context<'_, Self>,
    ) {
        if !self.expanded_projects.remove(id) {
            self.expanded_projects.insert(id.to_owned());
        }
        cx.notify();
    }

    pub(super) fn project_new_task(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.user_menu = false;
        self.project_menu = None;
        self.expanded_projects.insert(id.to_owned());
        self.push_section(Section::Studio, cx);
        let created = self.studio.update(cx, |panel, cx| {
            panel.open_project(id, cx)?;
            panel.create_task(cx)
        });
        if let Err(key) = created {
            self.studio.update(cx, |panel, cx| panel.note(key, cx));
        } else {
            let prompt = self.studio.read(cx).prompt.read(cx).focus_handle(cx);
            window.focus(&prompt, cx);
        }
        cx.notify();
    }

    pub(super) fn project_pin(&mut self, id: &str, cx: &mut Context<'_, Self>) {
        let pinned = self
            .studio
            .read(cx)
            .projects
            .iter()
            .find(|project| project.id == id)
            .is_some_and(|project| !project.pinned);
        let result = self
            .studio
            .update(cx, |panel, cx| panel.set_project_pinned(id, pinned, cx));
        self.project_menu = None;
        if let Err(key) = result {
            self.studio.update(cx, |panel, cx| panel.note(key, cx));
        }
        cx.notify();
    }

    pub(super) fn view_project(
        &mut self,
        id: &str,
        cx: &mut Context<'_, Self>,
    ) {
        let result = self.studio.read(cx).view_project(id);
        self.project_menu = None;
        if let Err(key) = result {
            self.studio.update(cx, |panel, cx| panel.note(key, cx));
        }
        cx.notify();
    }

    pub(super) fn request_project_delete(
        &mut self,
        id: &str,
        cx: &mut Context<'_, Self>,
    ) {
        self.project_menu = None;
        self.project_create.deleting_project_id = Some(id.to_owned());
        self.activate_dialog(state::ActiveDialog::ProjectDelete, cx);
    }

    fn close_project_delete(&mut self, cx: &mut Context<'_, Self>) {
        self.project_create.deleting_project_id = None;
        self.active_dialog = state::ActiveDialog::None;
        cx.notify();
    }

    fn confirm_project_delete(&mut self, cx: &mut Context<'_, Self>) {
        let Some(id) = self.project_create.deleting_project_id.clone() else {
            return;
        };
        let result = self
            .studio
            .update(cx, |panel, cx| panel.remove_project(&id, cx));
        self.close_project_delete(cx);
        self.expanded_projects.remove(&id);
        if let Err(key) = result {
            self.studio.update(cx, |panel, cx| panel.note(key, cx));
        }
    }

    fn choose_project_folder(&mut self, cx: &Context<'_, Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(self.t("project.folder").into()),
        });
        self.project_create.picker_generation =
            self.project_create.picker_generation.wrapping_add(1);
        let generation = self.project_create.picker_generation;
        cx.spawn(async move |weak, cx| {
            let result = picked.await;
            let _ = weak.update(cx, |app, cx| {
                if app.active_dialog != state::ActiveDialog::ProjectCreate
                    || app.project_create.picker_generation != generation
                {
                    return;
                }
                match result {
                    Ok(Ok(Some(mut paths))) => {
                        if let Some(folder) = paths.pop() {
                            if folder.is_dir() {
                                app.project_create.folder = Some(folder);
                                app.project_create.status = None;
                            } else {
                                app.project_create.status =
                                    Some("project.invalid_folder");
                            }
                        }
                    }
                    Ok(Ok(None)) => {}
                    _ => {
                        app.project_create.status =
                            Some("project.invalid_folder");
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn submit_project_create(&mut self, cx: &mut Context<'_, Self>) {
        let name = self.project_create.name.read(cx).as_str().trim().to_owned();
        let Some(folder) = self.project_create.folder.clone() else {
            self.project_create.status = Some("project.invalid_folder");
            cx.notify();
            return;
        };
        if name.is_empty() {
            self.project_create.status = Some("project.name_required");
            cx.notify();
            return;
        }
        let result =
            if let Some(id) = self.project_create.editing_project_id.clone() {
                self.studio.update(cx, |panel, cx| {
                    panel.update_project(&id, &name, &folder, cx).map(|()| ())
                })
            } else {
                self.studio.update(cx, |panel, cx| {
                    panel.create_project(&name, &folder, cx).map(|_| ())
                })
            };
        match result {
            Ok(_) => {
                if let Some(project_id) =
                    self.studio.read(cx).active_project_id.clone()
                {
                    self.expanded_projects.insert(project_id);
                }
                self.project_create.picker_generation =
                    self.project_create.picker_generation.wrapping_add(1);
                self.active_dialog = state::ActiveDialog::None;
                self.push_section(Section::Studio, cx);
                cx.notify();
            }
            Err(key) => {
                self.project_create.status = Some(key);
                cx.notify();
            }
        }
    }

    fn close_project_create(&mut self, cx: &mut Context<'_, Self>) {
        self.project_create.picker_generation =
            self.project_create.picker_generation.wrapping_add(1);
        self.project_create.editing_project_id = None;
        self.project_create.title_key = "project.create_title";
        self.active_dialog = state::ActiveDialog::None;
        cx.notify();
    }

    pub(super) fn projects_layer(
        &self,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        theme::modal_layer(
            "project-create-layer",
            &self.theme,
            &self.ui,
            self.project_create_dialog(window, cx),
        )
    }

    fn project_create_dialog(
        &self,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let name = self.project_create.name.read(cx).as_str().trim();
        let can_create =
            !name.is_empty() && self.project_create.folder.is_some();
        let folder = self.project_create.folder.as_ref();
        let frame = self.theme.modal_size(window.viewport_size());
        theme::dialog(
            "project-create-card",
            self.t(self.project_create.title_key),
            &self.theme,
            &self.ui,
            cx,
        )
        .w(px(360.))
        .max_w(frame.width)
        .max_h(frame.height)
        .overflow_y_scroll()
        .bg(self.theme.canvas())
        .rounded(self.theme.modal_radius())
        .shadow_md()
        .px(px(18.))
        .pt_3()
        .pb(px(16.))
        .flex()
        .flex_col()
        .gap_0()
        .on_click(cx.listener(|_app, _event, _window, cx| {
            cx.stop_propagation();
        }))
        .child(self.project_create_close(cx))
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .size(px(40.))
                        .rounded(px(12.))
                        .bg(self.theme.tab_selected_bg())
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            theme::icons::briefcase_business().size(px(18.)),
                        ),
                )
                .child(
                    div()
                        .text_size(px(18.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(self.theme.ink())
                        .child(self.t(self.project_create.title_key)),
                ),
        )
        .child(
            div()
                .mt(px(20.))
                .w_full()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(self.project_field_label("project.name"))
                        .child(
                            theme::TextField::new(
                                "project-name",
                                self.t("project.name"),
                                self.project_create.name.clone(),
                            )
                            .placeholder(self.t("project.name_placeholder"))
                            .quiet()
                            .render(&self.theme, cx)
                            .min_h(px(34.)),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(self.project_field_label("project.folder"))
                        .child(self.project_folder_button(folder, cx)),
                ),
        )
        .children(self.project_create.status.map(|key| {
            let message = self.t(key);
            div()
                .id("project-create-error")
                .role(gpui::Role::Alert)
                .aria_live(gpui::accesskit::Live::Assertive)
                .text_size(self.theme.caption_size())
                .text_color(self.theme.danger())
                .whitespace_normal()
                .child(message)
        }))
        .child(
            div()
                .mt(px(16.))
                .w_full()
                .flex()
                .gap_3()
                .child(
                    theme::Button::new(
                        "cancel-project-create",
                        self.t("more.cancel"),
                    )
                    .variant(theme::ControlVariant::Secondary)
                    .full_width()
                    .render(
                        &self.theme,
                        cx.listener(|app, _event, _window, cx| {
                            cx.stop_propagation();
                            app.close_project_create(cx);
                        }),
                    )
                    .min_h(px(34.))
                    .border_1()
                    .border_color(self.theme.line())
                    .bg(self.theme.canvas()),
                )
                .child(
                    theme::Button::new(
                        "submit-project-create",
                        self.t(
                            if self.project_create.editing_project_id.is_some()
                            {
                                "project.save"
                            } else {
                                "project.create"
                            },
                        ),
                    )
                    .variant(theme::ControlVariant::Primary)
                    .disabled(!can_create)
                    .full_width()
                    .render(
                        &self.theme,
                        cx.listener(|app, _event, _window, cx| {
                            cx.stop_propagation();
                            app.submit_project_create(cx);
                        }),
                    )
                    .min_h(px(34.)),
                ),
        )
    }

    fn project_create_close(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div().w_full().h(px(24.)).flex().justify_end().child(
            theme::IconButton::new(
                "close-project-create",
                self.t("ui.close"),
                theme::icons::x(),
            )
            .size(theme::ControlSize::Small)
            .render(
                &self.theme,
                cx.listener(|app, _event, _window, cx| {
                    cx.stop_propagation();
                    app.close_project_create(cx);
                }),
            ),
        )
    }

    fn project_field_label(&self, key: &'static str) -> impl IntoElement {
        div()
            .text_size(self.theme.body_size())
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(self.theme.ink())
            .child(self.t(key))
    }

    fn project_folder_button(
        &self,
        folder: Option<&std::path::PathBuf>,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let label = folder.map_or_else(
            || self.t("project.folder"),
            |path| path.display().to_string(),
        );
        theme::interactive("choose-project-folder", label.clone(), &self.theme)
            .w_full()
            .min_h(px(34.))
            .rounded_lg()
            .bg(self.theme.library_selected())
            .px_3()
            .flex()
            .items_center()
            .gap_3()
            .text_color(if folder.is_some() {
                self.theme.ink()
            } else {
                self.theme.muted()
            })
            .child(theme::icons::folder().size(px(14.)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .whitespace_nowrap()
                    .truncate()
                    .when(folder.is_none(), |text| {
                        text.text_size(self.theme.body_size())
                    })
                    .child(label),
            )
            .on_click(cx.listener(|app, _event, _window, cx| {
                app.choose_project_folder(cx);
            }))
    }

    pub(super) fn project_menu_content(
        &self,
        project_id: &str,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let project = self
            .studio
            .read(cx)
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .cloned();
        let pinned = project.as_ref().is_some_and(|project| project.pinned);
        let pin_id = project_id.to_owned();
        let edit_id = project_id.to_owned();
        let copy_id = project_id.to_owned();
        let view_id = project_id.to_owned();
        let collapse_id = project_id.to_owned();
        let delete_id = project_id.to_owned();
        let pin_label = self.t(if pinned {
            "project.unpin"
        } else {
            "project.pin"
        });
        let edit_label = self.t("project.edit");
        let copy_label = self.t("project.copy");
        let view_label = self.t("project.view");
        let show_label =
            self.t(if self.expanded_projects.contains(project_id) {
                "project.show_less"
            } else {
                "project.show_more"
            });
        let delete_label = self.t("project.delete");
        let width = project_menu_width(
            window,
            &[
                pin_label.as_str(),
                edit_label.as_str(),
                copy_label.as_str(),
                view_label.as_str(),
                show_label.as_str(),
                delete_label.as_str(),
            ],
            self.theme.body_size(),
        );
        theme::menu(
            "project-menu-content",
            self.t("dock.projects"),
            &self.theme,
            &self.ui,
            cx,
        )
        .w(width)
        .p_1()
        .gap_1()
        .on_click(
            cx.listener(|_app, _event, _window, cx| cx.stop_propagation()),
        )
        .child(self.project_menu_row(
            "project-pin",
            theme::icons::pin(),
            pin_label,
            false,
            cx,
            move |app, cx| app.project_pin(&pin_id, cx),
        ))
        .child(self.project_menu_row(
            "project-edit",
            theme::icons::folder_pen(),
            edit_label,
            false,
            cx,
            move |app, cx| app.edit_project(&edit_id, cx),
        ))
        .child(self.project_menu_row(
            "project-copy",
            theme::icons::copy(),
            copy_label,
            false,
            cx,
            move |app, cx| app.copy_project(&copy_id, cx),
        ))
        .child(self.project_menu_row(
            "project-view",
            theme::icons::eye(),
            view_label,
            false,
            cx,
            move |app, cx| app.view_project(&view_id, cx),
        ))
        .child(self.project_menu_row(
            "project-show-less",
            theme::icons::chevrons_up_down(),
            show_label,
            false,
            cx,
            move |app, cx| {
                app.project_menu = None;
                app.toggle_project_expanded(&collapse_id, cx);
            },
        ))
        .child(div().my_1().mx_1().h(px(1.)).bg(self.theme.line()))
        .child(self.project_menu_row(
            "project-delete",
            theme::icons::trash(),
            delete_label,
            true,
            cx,
            move |app, cx| app.request_project_delete(&delete_id, cx),
        ))
    }

    fn project_menu_row(
        &self,
        id: &'static str,
        icon: theme::icons::Icon,
        label: String,
        danger: bool,
        cx: &Context<'_, Self>,
        action: impl Fn(&mut FlowyApp, &mut Context<'_, FlowyApp>) + 'static,
    ) -> impl IntoElement {
        theme::interactive(id, label.clone(), &self.theme)
            .w_full()
            .min_h(px(32.))
            .rounded_md()
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .hover(|row| row.bg(self.theme.account_menu_hover()))
            .child(icon)
            .text_color(if danger {
                self.theme.danger()
            } else {
                self.theme.ink()
            })
            .child(
                div()
                    .flex_shrink_0()
                    .whitespace_nowrap()
                    .text_size(self.theme.body_size())
                    .text_color(if danger {
                        self.theme.danger()
                    } else {
                        self.theme.ink()
                    })
                    .child(label),
            )
            .on_click(cx.listener(move |app, _event, _window, cx| {
                cx.stop_propagation();
                action(app, cx);
            }))
    }

    pub(super) fn project_delete_layer(
        &self,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let frame = self.theme.modal_size(window.viewport_size());
        let name = self
            .project_create
            .deleting_project_id
            .as_ref()
            .and_then(|id| {
                self.studio
                    .read(cx)
                    .projects
                    .iter()
                    .find(|project| &project.id == id)
            })
            .map_or_else(String::new, |project| project.name.clone());
        theme::modal_layer(
            "project-delete-layer",
            &self.theme,
            &self.ui,
            theme::dialog(
                "project-delete-card",
                self.t("project.delete"),
                &self.theme,
                &self.ui,
                cx,
            )
            .w(px(360.))
            .max_w(frame.width)
            .bg(self.theme.canvas())
            .shadow_md()
            .px_5()
            .py_4()
            .flex()
            .flex_col()
            .gap_4()
            .on_click(
                cx.listener(|_app, _event, _window, cx| cx.stop_propagation()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(self.theme.heading_size())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.tf(
                                "project.delete_title",
                                &[("name", &name)],
                            )),
                    )
                    .child(
                        theme::IconButton::new(
                            "close-project-delete",
                            self.t("ui.close"),
                            theme::icons::x(),
                        )
                        .size(theme::ControlSize::Small)
                        .render(
                            &self.theme,
                            cx.listener(|app, _event, _window, cx| {
                                app.close_project_delete(cx)
                            }),
                        ),
                    ),
            )
            .child(
                div()
                    .text_size(self.theme.body_size())
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(self.t("project.delete_notice")),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .gap_3()
                    .child(
                        theme::Button::new(
                            "cancel-project-delete",
                            self.t("more.cancel"),
                        )
                        .variant(theme::ControlVariant::Secondary)
                        .full_width()
                        .render(
                            &self.theme,
                            cx.listener(|app, _event, _window, cx| {
                                app.close_project_delete(cx)
                            }),
                        ),
                    )
                    .child(
                        theme::Button::new(
                            "confirm-project-delete",
                            self.t("project.delete"),
                        )
                        .variant(theme::ControlVariant::Danger)
                        .full_width()
                        .render(
                            &self.theme,
                            cx.listener(|app, _event, _window, cx| {
                                app.confirm_project_delete(cx)
                            }),
                        ),
                    ),
            ),
        )
    }
}

/// Width of the project menu so every label fits on one line.
///
/// Chrome is the menu border and padding, the row border and padding, the
/// 16px icon, and the row gap. `border_1` / `border_2` are pixels; padding and
/// gap are rems.
fn project_menu_width(
    window: &Window,
    labels: &[&str],
    font_size: Pixels,
) -> Pixels {
    let font = theme::ui_font();
    let text = labels
        .iter()
        .map(|label| {
            let run = TextRun {
                len: label.len(),
                font: font.clone(),
                ..Default::default()
            };
            window
                .text_system()
                .layout_line(*label, font_size, &[run])
                .width
        })
        .max()
        .unwrap_or(px(0.));
    let rem = window.rem_size();
    text + px(30.) + rem * 2.
}
