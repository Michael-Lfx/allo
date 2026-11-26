use super::{FlowyApp, InstallPhase};
use crate::app::{
    Section,
    state::{ActiveDialog, MySkillsTab},
};
use flowy_agent::InstalledSkill;
use flowy_ui as theme;
use gpui::{
    Context, Div, FontWeight, Hsla, Pixels, SharedString, Window, div,
    prelude::*, px,
};

use gpui_kit::base::input::{InputAccessibility, InputEditorStyle};

const CARD_MIN_WIDTH: f32 = 340.;
const CARD_GAP: f32 = 16.;

impl FlowyApp {
    pub(super) fn open_my_skills(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let placeholder = SharedString::from(self.t("skills.library_search"));
        let ink = Hsla::from(self.theme.ink());
        let muted = Hsla::from(self.theme.muted());
        self.skills.mine_query.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.set_placeholder(placeholder.clone(), window, cx);
            input.set_editor_style(InputEditorStyle {
                foreground: ink,
                muted_foreground: muted,
                caret: ink,
                ..Default::default()
            });
            input.set_accessibility(
                InputAccessibility::new(Some(gpui::Role::TextInput))
                    .with_label(Some(placeholder.clone()))
                    .with_placeholder(Some(placeholder)),
            );
        });
        self.skills.mine_tab = MySkillsTab::Installed;
        self.skills.mine_selected = None;
        self.push_section(Section::MySkills, cx);
    }

    pub(in crate::app) fn my_skills_canvas(
        &self,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let viewport_width = window.viewport_size().width;
        let inset = theme::page_inset(viewport_width);
        let dock_width = if self.dock_visible {
            px(theme::DOCK_WIDTH)
        } else {
            px(0.)
        };
        let content_width = (viewport_width - dock_width)
            .min(px(theme::PAGE_MAX_WIDTH))
            - inset * 2.;
        let selected = self
            .skills
            .mine_selected
            .as_ref()
            .and_then(|(owner, slug)| self.installed_skill_for(owner, slug));
        div()
            .id("my-skills-page")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .w_full()
                    .max_w(px(theme::PAGE_MAX_WIDTH))
                    .mx_auto()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .px(inset)
                    .pt_8()
                    .child(if let Some(skill) = selected {
                        self.my_skill_details(skill, cx).into_any_element()
                    } else {
                        self.my_skills_collection(content_width, cx)
                            .into_any_element()
                    }),
            )
    }

    fn my_skills_heading(&self, cx: &Context<'_, Self>) -> Div {
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                div()
                    .text_size(self.theme.heading_size() + px(8.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.t("skills.mine")),
            )
            .child(self.my_skills_explore(cx))
    }

    fn my_skills_explore(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        theme::Button::new(
            "my-skills-explore",
            self.t("skills.library_explore"),
        )
        .framed()
        .trailing_icon(theme::icons::arrow_right().size(px(14.)))
        .render(
            &self.theme,
            cx.listener(|app, _event, _window, cx| {
                app.select_navigation("skills", Section::Skills, cx);
            }),
        )
    }

    fn my_skills_collection(
        &self,
        content_width: Pixels,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let card_gap = px(CARD_GAP);
        let card_width = if content_width >= px(CARD_MIN_WIDTH * 2.) + card_gap
        {
            ((content_width - card_gap) / 2.).floor()
        } else {
            content_width.max(px(0.))
        };
        let query = self
            .skills
            .mine_query
            .read(cx)
            .value()
            .trim()
            .to_lowercase();
        let tab = self.skills.mine_tab;
        let mut cards = div()
            .flex()
            .flex_wrap()
            .gap(card_gap)
            .items_stretch()
            .pb_8();
        let mut empty = true;
        match tab {
            MySkillsTab::Installed => {
                for skill in &self.skills.installed {
                    if matches_query(
                        &query,
                        &[
                            &skill.name,
                            &skill.description,
                            &skill.id.slug,
                            &skill.id.owner_handle,
                        ],
                    ) {
                        empty = false;
                        cards = cards.child(
                            self.my_installed_card(skill, card_width, cx),
                        );
                    }
                }
            }
            MySkillsTab::Saved => {
                for key in &self.prefs.skills {
                    if matches_query(&query, &[key]) {
                        empty = false;
                        cards = cards
                            .child(self.my_saved_card(key, card_width, cx));
                    }
                }
            }
        }
        let tabs =
            theme::FilterTabs::new("my-skills-tabs", self.t("skills.mine"));
        let tabs = [
            (
                MySkillsTab::Installed,
                "my-skills-installed",
                "skills.library_installed",
            ),
            (MySkillsTab::Saved, "my-skills-saved", "skills.saved"),
        ]
        .into_iter()
        .fold(tabs, |tabs, (value, id, key)| {
            tabs.child(
                theme::filter_chip(id, self.t(key), tab == value, &self.theme)
                    .on_click(cx.listener(move |app, _event, _window, cx| {
                        app.skills.mine_tab = value;
                        app.skills.mine_selected = None;
                        cx.notify();
                    })),
            )
        });
        let empty_key = if !query.is_empty() {
            "skills.empty"
        } else if tab == MySkillsTab::Saved {
            "skills.library_no_saved"
        } else {
            "skills.no_installed"
        };
        let body = if empty {
            let icon = if !query.is_empty() {
                theme::icons::search()
            } else if tab == MySkillsTab::Saved {
                theme::icons::star()
            } else {
                theme::icons::puzzle()
            };
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .items_center()
                .min_h(px(160.))
                .justify_center()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_3()
                        .pb_8()
                        .text_center()
                        .child(self.my_skill_mark(icon))
                        .child(
                            div()
                                .text_size(self.theme.body_size())
                                .text_color(self.theme.muted())
                                .child(self.t(empty_key)),
                        ),
                )
                .into_any_element()
        } else {
            div()
                .id(if tab == MySkillsTab::Saved {
                    "my-skills-saved-scroll"
                } else {
                    "my-skills-installed-scroll"
                })
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(cards)
                .into_any_element()
        };
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap_5()
            .child(self.my_skills_heading(cx))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .pb_3()
                    .border_b_1()
                    .border_color(self.theme.line())
                    .flex_shrink_0()
                    .child(tabs.render().w_auto().max_w_full())
                    .child(self.my_skills_toolbar(tab, cx)),
            )
            .child(body)
    }

    fn my_skills_toolbar(
        &self,
        tab: MySkillsTab,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let search = theme::ToolbarSearch::new(
            "my-skills-search",
            self.t("skills.library_search"),
            self.skills.mine_query.clone(),
        )
        .render(&self.theme);
        div()
            .min_w_0()
            .max_w_full()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .when(
                tab == MySkillsTab::Installed
                    && !self.skills.installed.is_empty(),
                |tools| {
                    tools.child(
                        theme::Button::new(
                            "my-skills-updates",
                            self.t("skills.check_updates"),
                        )
                        .variant(theme::ControlVariant::Ghost)
                        .icon(theme::icons::refresh_cw().size(px(14.)))
                        .size(theme::ControlSize::Small)
                        .render(
                            &self.theme,
                            cx.listener(|app, _event, _window, cx| {
                                app.check_skill_updates(cx);
                            }),
                        ),
                    )
                },
            )
            .child(search.max_w_full())
    }

    fn my_skill_mark(&self, icon: theme::icons::Icon) -> Div {
        div()
            .size(px(42.))
            .rounded_lg()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(self.theme.tab_selected_bg())
            .child(icon.size(px(21.)).text_color(self.theme.ink()))
    }

    fn my_skill_card(&self, key: String, width: Pixels) -> gpui::Stateful<Div> {
        div()
            .id(SharedString::from(key))
            .w(width)
            .max_w_full()
            .min_w_0()
            .rounded_xl()
            .bg(self.theme.card())
            .border_1()
            .border_color(self.theme.line())
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
    }

    fn my_skill_name(&self, name: String) -> Div {
        div()
            .min_w_0()
            .truncate()
            .text_size(self.theme.body_size() + px(4.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(name)
    }

    fn my_installed_card(
        &self,
        skill: &InstalledSkill,
        width: Pixels,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let owner = &skill.id.owner_handle;
        let slug = &skill.id.slug;
        let identity = (owner.clone(), slug.clone());
        let use_identity = identity.clone();
        self.my_skill_card(format!("my-skill-{owner}/{slug}"), width)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(self.my_skill_mark(installed_icon(skill)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(self.my_skill_name(skill.name.clone()))
                            .child(
                                div()
                                    .text_size(self.theme.caption_size())
                                    .text_color(self.theme.muted())
                                    .child(self.t(origin_key(skill))),
                            ),
                    )
                    .child(
                        theme::IconButton::new(
                            "my-skill-manage",
                            self.t("settings.manage"),
                            theme::icons::ellipsis(),
                        )
                        .size(theme::ControlSize::Small)
                        .render(
                            &self.theme,
                            cx.listener(move |app, _event, _window, cx| {
                                app.skills.mine_selected =
                                    Some(identity.clone());
                                cx.notify();
                            }),
                        ),
                    ),
            )
            .child(
                div()
                    .text_size(self.theme.body_size())
                    .text_color(self.theme.muted())
                    .line_clamp(2)
                    .child(skill.description.clone()),
            )
            .child(
                div().mt_auto().flex().items_center().justify_end().child(
                    theme::Button::new(
                        "my-skill-use",
                        self.t("skills.try_out"),
                    )
                    .variant(theme::ControlVariant::Ghost)
                    .size(theme::ControlSize::Small)
                    .trailing_icon(theme::icons::arrow_right().size(px(14.)))
                    .render(
                        &self.theme,
                        cx.listener(move |app, _event, _window, cx| {
                            if let Some(skill) = app
                                .installed_skill_for(
                                    &use_identity.0,
                                    &use_identity.1,
                                )
                                .cloned()
                            {
                                app.try_installed_skill(&skill, cx);
                            }
                        }),
                    ),
                ),
            )
    }

    fn my_saved_card(
        &self,
        key: &str,
        width: Pixels,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let address = key.split_once('/');
        let name = address.map_or(key, |(_, slug)| slug);
        let remove = key.to_owned();
        self.my_skill_card(format!("my-saved-skill-{key}"), width)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(self.my_skill_mark(theme::icons::star()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(self.my_skill_name(name.to_owned()))
                            .children(address.map(|(owner, _)| {
                                div()
                                    .truncate()
                                    .text_size(self.theme.caption_size())
                                    .text_color(self.theme.muted())
                                    .child(owner.to_owned())
                            })),
                    )
                    .child(
                        theme::IconButton::new(
                            "my-skill-remove",
                            self.t("skills.remove_saved"),
                            theme::icons::x(),
                        )
                        .size(theme::ControlSize::Small)
                        .render(
                            &self.theme,
                            cx.listener(move |app, _event, _window, cx| {
                                app.toggle_saved_skill(remove.clone(), cx);
                            }),
                        ),
                    ),
            )
            .when(
                self.installed_skill_for(
                    address.map_or("flowy", |(owner, _)| owner),
                    name,
                )
                .is_some(),
                |card| {
                    card.child(
                        div()
                            .mt_auto()
                            .text_color(self.theme.muted())
                            .text_size(self.theme.caption_size())
                            .child(self.t("skills.library_installed")),
                    )
                },
            )
            .when_some(
                address.filter(|(owner, slug)| {
                    self.installed_skill_for(owner, slug).is_none()
                }),
                |card, (owner, slug)| {
                    card.child(div().mt_auto().flex().justify_end().child(
                        self.my_skill_install_action(
                            key,
                            owner,
                            slug,
                            flowy_skills::UNVERSIONED,
                            "skills.install",
                            cx,
                        ),
                    ))
                },
            )
    }

    fn my_skill_install_action(
        &self,
        key: &str,
        owner: &str,
        slug: &str,
        version: &str,
        label: &'static str,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let progress = self
            .skills
            .install
            .as_ref()
            .filter(|install| install.key == key)
            .map(|install| &install.phase);
        let busy = self.skills.install.as_ref().is_some_and(|install| {
            !matches!(install.phase, InstallPhase::Failed(_))
        });
        let (key, owner, slug, version) = (
            key.to_owned(),
            owner.to_owned(),
            slug.to_owned(),
            version.to_owned(),
        );
        let loading = progress
            .is_some_and(|phase| !matches!(phase, InstallPhase::Failed(_)));
        let label = progress.filter(|_| loading).map_or_else(
            || self.t(label),
            |phase| self.install_phase_label(phase),
        );
        div()
            .flex()
            .flex_wrap()
            .gap_2()
            .items_center()
            .children(
                progress
                    .filter(|phase| matches!(phase, InstallPhase::Failed(_)))
                    .map(|phase| {
                        div()
                            .text_size(self.theme.caption_size())
                            .text_color(self.theme.danger())
                            .child(self.install_phase_label(phase))
                    }),
            )
            .child(
                theme::Button::new("my-skills-install", label)
                    .framed()
                    .size(theme::ControlSize::Small)
                    .loading(loading)
                    .disabled(busy && !loading)
                    .render(
                        &self.theme,
                        cx.listener(move |app, _event, _window, cx| {
                            app.install_shelf_skill(
                                &owner, &slug, &version, &key, cx,
                            );
                        }),
                    ),
            )
    }

    fn my_skill_details(
        &self,
        skill: &InstalledSkill,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let auto_id = skill.id.clone();
        let scripts_id = skill.id.clone();
        let uninstall_id = skill.id.clone();
        let key = format!("{}/{}", skill.id.owner_handle, skill.id.slug);
        let newer = skill
            .latest_version
            .as_deref()
            .filter(|latest| *latest != skill.id.version);
        let auto_label = self.t(if skill.auto_invoke {
            "skills.auto_on"
        } else {
            "skills.auto_off"
        });
        let scripts_label = self.t(if skill.scripts_enabled {
            "skills.scripts_on"
        } else {
            "skills.scripts_off"
        });
        let back = theme::Button::new("my-skill-back", self.t("skills.mine"))
            .icon(theme::icons::arrow_left().size(px(14.)))
            .variant(theme::ControlVariant::Ghost)
            .size(theme::ControlSize::Small)
            .render(
                &self.theme,
                cx.listener(|app, _event, _window, cx| {
                    app.skills.mine_selected = None;
                    cx.notify();
                }),
            );
        let auto = theme::Switch::new(
            "my-skill-auto",
            auto_label.clone(),
            skill.auto_invoke,
        )
        .render(
            &self.theme,
            cx.listener(move |app, enabled, _window, cx| {
                app.set_auto_invoke(auto_id.clone(), *enabled, cx);
            }),
        );
        let scripts = theme::Switch::new(
            "my-skill-scripts",
            scripts_label.clone(),
            skill.scripts_enabled,
        )
        .render(
            &self.theme,
            cx.listener(move |app, enabled, _window, cx| {
                if *enabled {
                    app.skills.script_confirm = Some(scripts_id.clone());
                    app.activate_dialog(ActiveDialog::SkillScriptConfirm, cx);
                } else {
                    app.set_scripts_enabled(scripts_id.clone(), false, cx);
                }
            }),
        );
        let actions = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .children(newer.map(|version| {
                self.my_skill_install_action(
                    &key,
                    &skill.id.owner_handle,
                    &skill.id.slug,
                    version,
                    "skills.update",
                    cx,
                )
            }))
            .when(
                skill.origin == flowy_skills::InstallOrigin::Community,
                |actions| {
                    actions.child(
                        theme::Button::new(
                            "my-skill-uninstall",
                            self.t("skills.uninstall"),
                        )
                        .variant(theme::ControlVariant::Ghost)
                        .size(theme::ControlSize::Small)
                        .render(
                            &self.theme,
                            cx.listener(move |app, _event, _window, cx| {
                                app.uninstall_skill(uninstall_id.clone(), cx);
                            }),
                        ),
                    )
                },
            );
        let heading = div()
            .flex()
            .items_center()
            .gap_3()
            .child(self.my_skill_mark(installed_icon(skill)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(self.my_skill_name(skill.name.clone()))
                    .child(
                        div()
                            .text_size(self.theme.caption_size())
                            .text_color(self.theme.muted())
                            .child(format!(
                                "{} · {}",
                                self.t(origin_key(skill)),
                                skill.id.version
                            )),
                    ),
            );
        div()
            .id("my-skill-details-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                div()
                    .w_full()
                    .max_w(px(540.))
                    .flex()
                    .flex_col()
                    .gap_4()
                    .pb_8()
                    .child(div().flex().child(back))
                    .child(heading)
                    .child(
                        div()
                            .text_size(self.theme.body_size())
                            .whitespace_normal()
                            .text_color(self.theme.muted())
                            .child(skill.description.clone()),
                    )
                    .child(self.my_skill_option(auto_label, auto))
                    .child(self.my_skill_option(scripts_label, scripts))
                    .child(actions),
            )
    }

    fn my_skill_option(&self, label: String, control: impl IntoElement) -> Div {
        div()
            .flex()
            .justify_between()
            .items_center()
            .gap_3()
            .pt_3()
            .border_t_1()
            .border_color(self.theme.line())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(self.theme.label_size())
                    .child(label),
            )
            .child(div().flex_shrink_0().child(control))
    }
}

fn matches_query(query: &str, fields: &[&str]) -> bool {
    query.is_empty()
        || fields
            .iter()
            .any(|field| field.to_lowercase().contains(query))
}

const fn origin_key(skill: &InstalledSkill) -> &'static str {
    match skill.origin {
        flowy_skills::InstallOrigin::Official => "skills.built_in",
        flowy_skills::InstallOrigin::Community => "skills.community",
    }
}

fn installed_icon(skill: &InstalledSkill) -> theme::icons::Icon {
    match (skill.id.owner_handle.as_str(), skill.id.slug.as_str()) {
        ("flowy", "excel") => theme::icons::layout_grid(),
        ("flowy", "pdf") => theme::icons::book(),
        _ => theme::icons::puzzle(),
    }
}
